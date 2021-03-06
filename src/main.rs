#[forbid(unsafe_code)]
use anyhow::bail;
use cloudflare::endpoints::{dns, zone};
use cloudflare::framework::apiclient::ApiClient;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::response::ApiSuccess;
use cloudflare::framework::{Environment, HttpApiClient, HttpApiClientConfig};
use log::debug;
use std::collections::HashMap;
use std::env;
use std::net::Ipv4Addr;
use structopt::StructOpt;

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
    #[structopt(short, long, help = "Dry run")]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();

    if env::var_os("RUST_LOG").is_none() {
        if opts.dry_run {
            env::set_var("RUST_LOG", "turbo_spoon=debug");
        } else {
            env::set_var("RUST_LOG", "turbo_spoon=info");
        }
    }

    pretty_env_logger::init();

    let ip_address = match public_ip::addr_v4().await {
        Some(ip_address) => ip_address,
        None => bail!("failed to determine public IPv4 address"),
    };

    debug!("public IPv4 address: {0}", &ip_address);

    let creds = Credentials::UserAuthToken { token: opts.token };
    let client = match HttpApiClient::new(
        creds,
        HttpApiClientConfig::default(),
        Environment::Production,
    ) {
        Ok(client) => client,
        Err(_e) => bail!("failed to initialize Cloudflare client"),
    };
    let my_client = MyClient(client);

    let zone_id = my_client.zone_name_to_id(&opts.zone)?;
    debug!("zone {0} (id: {1}) found", &opts.zone, &zone_id);

    let record_names: Vec<_> = opts.records.split(",").map(String::from).collect();
    let record_map = my_client.record_names_to_ids(zone_id.clone(), record_names)?;
    for (ref record_name, ref record_id) in &record_map {
        debug!(
            "DNS record {2} (id: {3}) found in {0} (id: {1})",
            opts.zone, zone_id, record_name, record_id
        )
    }

    let results = my_client.update_dns_records(&zone_id, &record_map, &ip_address)?;
    for result in results {
        match result.content {
            dns::DnsContent::A { ref content } => {
                debug!("DNS record {0} is changed to {1}", result.name, content)
            }
            _ => (),
        }
    }

    Ok(())
}

struct MyClient(HttpApiClient);

impl MyClient {
    fn record_names_to_ids<S: AsRef<str>>(
        &self,
        zone_id: S,
        record_names: Vec<String>,
    ) -> anyhow::Result<HashMap<String, String>> {
        let mut record_map: HashMap<String, String> = HashMap::new();

        for record_name in record_names {
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

            let result: ApiSuccess<Vec<dns::DnsRecord>> = self.0.request(&params)?;
            let record_id = match result.result.first() {
                Some(dns_record) => dns_record.id.clone(),
                None => bail!("DNS record {0} does not exist", record_name),
            };
            record_map.insert(record_name.clone(), record_id);
        }

        Ok(record_map)
    }

    fn update_dns_records<S: AsRef<str>>(
        &self,
        zone_id: S,
        record_map: &HashMap<String, String>,
        ip_address: &Ipv4Addr,
    ) -> anyhow::Result<Vec<dns::DnsRecord>> {
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

            let result: ApiSuccess<dns::DnsRecord> = self.0.request(&params)?;
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
        let result: ApiSuccess<Vec<zone::Zone>> = self.0.request(&params)?;
        match result.result.first() {
            Some(zone) => Ok(zone.id.clone()),
            None => bail!("zone {0} does not exist", zone_name),
        }
    }
}
