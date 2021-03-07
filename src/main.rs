#[forbid(unsafe_code)]
use std::collections::HashMap;
use std::env;
use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::bail;
use cloudflare::endpoints::{dns, zone};
use cloudflare::framework::apiclient::ApiClient;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::response::ApiSuccess;
use cloudflare::framework::{Environment, HttpApiClient, HttpApiClientConfig};
use log::{debug, info};
use structopt::StructOpt;
use tokio::time;

#[derive(StructOpt)]
#[structopt(about, author)]
struct Opts {
    #[structopt(short, long, help = "Cloudflare token", env = "CLOUDFLARE_TOKEN")]
    token: String,
    #[structopt(short, long, help = "Cloudflare zone name", env = "CLOUDFLARE_ZONE")]
    zone: String,
    #[structopt(
        short,
        long,
        help = "Cloudflare records separated with comma e.g. a.x.com,b.x.com",
        env = "CLOUDFLARE_RECORDS"
    )]
    records: String,
    #[structopt(long, help = "Debug mode")]
    debug: bool,
    #[structopt(short, long, help = "Daemon mode", env = "DAEMON")]
    daemon: bool,
    #[structopt(
        short,
        long,
        help = "Interval in seconds. Only in effect in daemon mode",
        default_value = "60"
    )]
    interval: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();

    if env::var_os("RUST_LOG").is_none() {
        if opts.debug {
            env::set_var("RUST_LOG", "turbo_spoon=debug");
        } else {
            env::set_var("RUST_LOG", "turbo_spoon=info");
        }
    }

    pretty_env_logger::init();

    let record_names: Vec<_> = opts.records.split(",").map(String::from).collect();
    let params = CFClientParams {
        token: opts.token,
        zone_name: opts.zone,
        record_names,
    };
    let cf_client = CFClient::new(params)?;

    if opts.daemon {
        run_daemon(&cf_client, opts.interval).await?;
    } else {
        run_once(&cf_client).await?;
    }

    Ok(())
}

async fn run_daemon(cf_client: &CFClient, seconds: u64) -> anyhow::Result<()> {
    info!("daemon starts, update for the first time");

    loop {
        info!("update DNS records");
        run_once(cf_client).await?;
        info!("done");
        info!("wait for {0} seconds", seconds);
        time::sleep(Duration::from_secs(seconds)).await;
    }
}

async fn run_once(cf_client: &CFClient) -> anyhow::Result<()> {
    let ip_address = match public_ip::addr_v4().await {
        Some(ip_address) => ip_address,
        None => bail!("failed to determine public IPv4 address"),
    };

    debug!("public IPv4 address: {0}", &ip_address);

    let results = cf_client.update_dns_records(&ip_address)?;
    for result in results {
        match result.content {
            dns::DnsContent::A { ref content } => {
                debug!("DNS record {0} refers to {1}", result.name, content)
            }
            _ => (),
        }
    }

    Ok(())
}

struct CFClientParams {
    token: String,
    zone_name: String,
    record_names: Vec<String>,
}

struct CFClient {
    params: CFClientParams,
    client: HttpApiClient,
}

impl CFClient {
    fn new(params: CFClientParams) -> anyhow::Result<Self> {
        let creds = Credentials::UserAuthToken {
            token: params.token.clone(),
        };
        let client = match HttpApiClient::new(
            creds,
            HttpApiClientConfig::default(),
            Environment::Production,
        ) {
            Ok(client) => client,
            Err(_e) => bail!("failed to initialize Cloudflare client"),
        };
        Ok(Self { params, client })
    }

    fn record_names_to_ids<S: AsRef<str>>(
        &self,
        zone_id: S,
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut record_map: HashMap<String, String> = HashMap::new();

        for record_name in &self.params.record_names {
            let params = dns::ListDnsRecords {
                zone_identifier: zone_id.as_ref(),
                params: dns::ListDnsRecordsParams {
                    record_type: None,
                    name: Some(record_name.clone()),
                    page: None,
                    per_page: None,
                    order: None,
                    direction: None,
                    search_match: None,
                },
            };

            let result: ApiSuccess<Vec<dns::DnsRecord>> = self.client.request(&params)?;
            let record_id = match result.result.first() {
                Some(dns_record) => dns_record.id.clone(),
                None => bail!("DNS record {0} does not exist", record_name),
            };
            record_map.insert(record_name.clone(), record_id);
        }

        Ok(record_map)
    }

    fn update_dns_records(&self, ip_address: &Ipv4Addr) -> anyhow::Result<Vec<dns::DnsRecord>> {
        let zone_id = self.zone_name_to_id(&self.params.zone_name)?;
        debug!("zone {0} (id: {1}) found", &self.params.zone_name, &zone_id);

        let record_map = self.record_names_to_ids(&zone_id)?;
        for (ref record_name, ref record_id) in &record_map {
            debug!(
                "DNS record {2} (id: {3}) found in {0} (id: {1})",
                self.params.zone_name, zone_id, record_name, record_id
            )
        }

        let mut results = vec![];

        for (ref record_name, ref record_id) in record_map {
            let params = dns::UpdateDnsRecord {
                zone_identifier: zone_id.as_ref(),
                identifier: record_id,
                params: dns::UpdateDnsRecordParams {
                    ttl: None,
                    proxied: None,
                    name: record_name,
                    content: dns::DnsContent::A {
                        content: ip_address.clone(),
                    },
                },
            };

            let result: ApiSuccess<dns::DnsRecord> = self.client.request(&params)?;
            results.push(result.result);
        }

        Ok(results)
    }

    fn zone_name_to_id<S: AsRef<str>>(&self, zone_name: S) -> anyhow::Result<String> {
        let zone_name = zone_name.as_ref();
        let params = zone::ListZones {
            params: zone::ListZonesParams {
                name: Some(zone_name.to_string()),
                status: None,
                page: None,
                per_page: None,
                order: None,
                direction: None,
                search_match: None,
            },
        };
        let result: ApiSuccess<Vec<zone::Zone>> = self.client.request(&params)?;
        match result.result.first() {
            Some(zone) => Ok(zone.id.clone()),
            None => bail!("zone {0} does not exist", zone_name),
        }
    }
}
