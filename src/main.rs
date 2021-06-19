#![forbid(unsafe_code)]
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use cloudflare::endpoints::dns::{
    DnsContent, DnsRecord, ListDnsRecords, ListDnsRecordsParams, UpdateDnsRecord,
    UpdateDnsRecordParams,
};
use cloudflare::endpoints::zone::{ListZones, ListZonesParams, Zone};
use cloudflare::framework::async_api::{ApiClient, Client};
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::response::{ApiFailure, ApiSuccess};
use cloudflare::framework::{Environment, HttpApiClientConfig};
use cron::Schedule;
use log::{debug, info};
use structopt::StructOpt;
use tokio::task::JoinHandle;
use tokio_retry::strategy::{jitter, ExponentialBackoff};

const HTTP_TIMEOUT: u64 = 30;

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
    /// Cron. Only in effect in daemon mode
    #[structopt(short, long, default_value = "0 */5 * * * * *", env = "CRON")]
    cron: String,
}

impl Opts {
    fn record_name_list(&self) -> Vec<String> {
        self.records.split(',').map(String::from).collect()
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

    if opts.daemon {
        run_daemon(&opts).await?;
    } else {
        run_once(&opts).await?;
    }

    Ok(())
}

async fn run_daemon(opts: &Opts) -> anyhow::Result<()> {
    let schedule = Schedule::from_str(&opts.cron)?;

    for datetime in schedule.upcoming(chrono::Utc) {
        info!("update DNS records at {}", datetime);

        loop {
            if chrono::Utc::now() > datetime {
                break;
            } else {
                tokio::time::sleep(Duration::from_millis(999)).await;
            }
        }

        let strategy = ExponentialBackoff::from_millis(10).map(jitter).take(3);
        let instant = Instant::now();
        tokio_retry::RetryIf::spawn(
            strategy,
            || run_once(opts),
            |e: &anyhow::Error| e.is::<ApiFailure>() || e.is::<PublicIPError>(),
        )
        .await?;

        let duration = Instant::now() - instant;
        info!("done in {}ms", duration.as_millis());
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct PublicIPError;

impl std::fmt::Display for PublicIPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to determine public IPv4 address")
    }
}

impl std::error::Error for PublicIPError {}

async fn run_once(opts: &Opts) -> anyhow::Result<()> {
    let ip_address = public_ip::addr_v4().await.ok_or(PublicIPError)?;

    let credentials = Credentials::UserAuthToken {
        token: opts.token.clone(),
    };
    let config = HttpApiClientConfig {
        http_timeout: Duration::from_secs(HTTP_TIMEOUT),
        ..Default::default()
    };
    let client = Arc::new(Client::new(credentials, config, Environment::Production)?);

    debug!("public IPv4 address: {}", &ip_address);

    let params = ListZones {
        params: ListZonesParams {
            name: Some(opts.zone.clone()),
            ..Default::default()
        },
    };
    let res: ApiSuccess<Vec<Zone>> = client.request(&params).await?;
    let zone_id = match res.result.first() {
        Some(zone) => zone.id.to_string(),
        None => bail!("zone not found: {}", opts.zone),
    };

    debug!("zone found: {} ({})", &opts.zone, &zone_id);

    let mut futs = vec![];
    for record_name in opts.record_name_list() {
        let client = client.clone();
        let zone_id = zone_id.clone();
        futs.push(tokio::spawn(async move {
            let params = ListDnsRecords {
                zone_identifier: &zone_id,
                params: ListDnsRecordsParams {
                    name: Some(record_name.clone()),
                    ..Default::default()
                },
            };
            let res: ApiSuccess<Vec<DnsRecord>> = client.request(&params).await?;
            let dns_record = match res.result.first() {
                Some(dns_record) => dns_record,
                None => bail!("DNS record not found: {}", record_name),
            };
            debug!("DNS record found: {} ({})", &record_name, &dns_record.id);
            Ok((dns_record.id.clone(), record_name))
        }));
    }

    let mut dns_record_ids = vec![];
    for fut in futs {
        let (dns_record_id, record_name) = fut.await??;
        dns_record_ids.push((dns_record_id, record_name));
    }

    let mut futs: Vec<JoinHandle<anyhow::Result<()>>> = vec![];
    for (dns_record_id, record_name) in dns_record_ids {
        let client = client.clone();
        let zone_id = zone_id.clone();
        futs.push(tokio::spawn(async move {
            let params = UpdateDnsRecord {
                zone_identifier: &zone_id,
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
            let res: ApiSuccess<DnsRecord> = client.request(&params).await?;
            let dns_record = res.result;
            let content = match dns_record.content {
                DnsContent::A { content } => content.to_string(),
                _ => "(not an A record)".into(),
            };
            debug!(
                "DNS record updated: {} ({}) -> {}",
                &record_name, &dns_record_id, content
            );

            Ok(())
        }));
    }

    for fut in futs {
        fut.await??;
    }

    Ok(())
}
