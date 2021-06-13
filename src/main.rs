#[forbid(unsafe_code)]
use std::env;
use std::time::Duration;

use anyhow::bail;
use cloudflare::endpoints::dns::{
    DnsContent, DnsRecord, ListDnsRecords, ListDnsRecordsParams, UpdateDnsRecord,
    UpdateDnsRecordParams,
};
use cloudflare::endpoints::zone::{ListZones, ListZonesParams, Zone};
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
    /// Cloudflare token
    #[structopt(short, long, env = "CLOUDFLARE_TOKEN")]
    token: String,
    /// Cloudflare zone name
    #[structopt(short, long, env = "CLOUDFLARE_ZONE")]
    zone: String,
    /// Cloudflare records separated with comma e.g. a.x.com,b.x.com
    #[structopt(short, long, env = "CLOUDFLARE_RECORDS")]
    records: String,
    /// Debug mode
    #[structopt(long)]
    debug: bool,
    /// Daemon mode
    #[structopt(short, long, env = "DAEMON")]
    daemon: bool,
    /// Interval in seconds. Only in effect in daemon mode
    #[structopt(short, long, default_value = "60", env = "INTERVAL")]
    interval: u64,
}

impl Opts {
    fn record_name_list(&self) -> Vec<String> {
        self.records.split(",").map(String::from).collect()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();

    if env::var_os("RUST_LOG").is_none() {
        if opts.debug {
            env::set_var("RUST_LOG", "cdu=debug");
        } else {
            env::set_var("RUST_LOG", "cdu=info");
        }
    }

    pretty_env_logger::init();

    let credentials = Credentials::UserAuthToken {
        token: opts.token.clone(),
    };
    let client = match HttpApiClient::new(
        credentials,
        HttpApiClientConfig::default(),
        Environment::Production,
    ) {
        Ok(c) => c,
        Err(e) => bail!("failed to initialize Cloudflare client: {:?}", e),
    };

    if opts.daemon {
        run_daemon(&opts, &client).await?;
    } else {
        run_once(&opts, &client).await?;
    }

    Ok(())
}

async fn run_daemon(opts: &Opts, client: &HttpApiClient) -> anyhow::Result<()> {
    info!("daemon starts, update for the first time");

    let interval = opts.interval;
    let duration = Duration::from_secs(interval);

    let mut timer = time::interval(duration);
    timer.tick().await; // tick for the first time
    loop {
        info!("update DNS records and timeout is {0} seconds", interval);
        let timeout_res = time::timeout(duration, run_once(opts, client)).await?;
        let _run_once_res = timeout_res?;
        info!("done. wait for next round");
        timer.tick().await; // wait for specific duration
    }
}

async fn run_once(opts: &Opts, client: &HttpApiClient) -> anyhow::Result<()> {
    let ip_address = match public_ip::addr_v4().await {
        Some(ip_address) => ip_address,
        None => bail!("failed to determine public IPv4 address"),
    };

    debug!("public IPv4 address: {0}", &ip_address);

    let params = ListZones {
        params: ListZonesParams {
            name: Some(opts.zone.clone()),
            ..Default::default()
        },
    };
    let res: ApiSuccess<Vec<Zone>> = client.request(&params)?;
    let zone = match res.result.first() {
        Some(zone) => zone,
        None => bail!("zone not found: {:?}", opts.zone),
    };

    debug!("zone found: {} ({})", &opts.zone, &zone.id);

    let mut dns_record_ids = vec![];
    for record_name in opts.record_name_list() {
        let params = ListDnsRecords {
            zone_identifier: &zone.id,
            params: ListDnsRecordsParams {
                name: Some(record_name.clone()),
                ..Default::default()
            },
        };
        let res: ApiSuccess<Vec<DnsRecord>> = client.request(&params)?;
        let dns_record = match res.result.first() {
            Some(dns_record) => dns_record,
            None => bail!("DNS record not found: {:?}", record_name),
        };
        debug!("DNS record found: {} ({})", &record_name, &dns_record.id);
        dns_record_ids.push((dns_record.id.clone(), record_name));
    }

    for (dns_record_id, record_name) in dns_record_ids {
        let params = UpdateDnsRecord {
            zone_identifier: &zone.id,
            identifier: &dns_record_id,
            params: UpdateDnsRecordParams {
                name: &record_name,
                content: DnsContent::A {
                    content: ip_address,
                },
                proxied: None,
                ttl: None,
            },
        };
        let res: ApiSuccess<DnsRecord> = client.request(&params)?;
        let dns_record = res.result;
        let content = match dns_record.content {
            DnsContent::A { content } => content.to_string(),
            _ => "(not an A record)".into(),
        };
        debug!(
            "DNS record updated: {} ({}) -> {}",
            &record_name, &dns_record_id, content
        );
    }

    Ok(())
}
