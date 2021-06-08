#[forbid(unsafe_code)]
use std::collections::HashMap;
use std::env;
use std::net::Ipv4Addr;
use std::time::Duration;

use anyhow::bail;
use log::{debug, info};
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;
use structopt::StructOpt;
use tokio::time;

#[cfg(test)]
use mockito;

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

    let record_names: Vec<_> = opts.records.split(",").map(String::from).collect();
    let params = CFClientParams {
        token: opts.token,
        zone_name: opts.zone,
        record_names,
        interval: opts.interval,
    };
    let cf_client = CFClient::new(params)?;

    if opts.daemon {
        run_daemon(&cf_client).await?;
    } else {
        run_once(&cf_client).await?;
    }

    Ok(())
}

async fn run_daemon(cf_client: &CFClient) -> anyhow::Result<()> {
    info!("daemon starts, update for the first time");

    let interval = cf_client.params.interval;
    let duration = Duration::from_secs(interval);

    let mut timer = time::interval(duration);
    timer.tick().await; // tick for the first time
    loop {
        info!("update DNS records and timeout is {0} seconds", interval);
        let timeout_res = time::timeout(duration, run_once(cf_client)).await?;
        let _run_once_res = timeout_res?;
        info!("done. wait for next round");
        timer.tick().await; // wait for specific duration
    }
}

async fn run_once(cf_client: &CFClient) -> anyhow::Result<()> {
    let ip_address = match public_ip::addr_v4().await {
        Some(ip_address) => ip_address,
        None => bail!("failed to determine public IPv4 address"),
    };

    debug!("public IPv4 address: {0}", &ip_address);

    let results = cf_client.update_dns_records(&ip_address).await?;
    for result in results {
        debug!("DNS record {0} refers to {1}", result.name, result.content);
    }

    Ok(())
}

#[derive(Default)]
struct CFClientParams {
    token: String,
    zone_name: String,
    record_names: Vec<String>,
    interval: u64,
}

struct CFClient {
    params: CFClientParams,
    client: Client,
}

#[derive(Deserialize)]
struct DnsRecord {
    id: String,
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct DnsRecords {
    result: Vec<DnsRecord>,
}

#[derive(Deserialize)]
struct UpdateDnsRecord {
    result: DnsRecord,
}

#[derive(Deserialize)]
struct Zone {
    id: String,
}

#[derive(Deserialize)]
struct Zones {
    result: Vec<Zone>,
}

impl CFClient {
    #[cfg(test)]
    fn endpoint() -> String {
        mockito::server_url()
    }

    #[cfg(not(test))]
    fn endpoint() -> String {
        "https://api.cloudflare.com/client/v4".into()
    }

    fn new(params: CFClientParams) -> anyhow::Result<Self> {
        let mut headers = header::HeaderMap::new();

        let authorization = format!("Bearer {0}", &params.token);
        let mut authorization = header::HeaderValue::from_str(&authorization)?;
        authorization.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, authorization);

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self { params, client })
    }

    async fn record_names_to_ids<S: AsRef<str>>(
        &self,
        zone_id: S,
    ) -> anyhow::Result<HashMap<String, String>> {
        let zone_id = zone_id.as_ref();
        let mut record_map: HashMap<String, String> = HashMap::new();
        for record_name in &self.params.record_names {
            let url = format!("{0}/zones/{1}/dns_records", Self::endpoint(), zone_id);
            let res: DnsRecords = self
                .client
                .get(&url)
                .query(&[("name", record_name)])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let record_id = match res.result.first() {
                Some(dns_record) => dns_record.id.clone(),
                None => bail!("DNS record {0} does not exist", record_name),
            };
            record_map.insert(record_name.clone(), record_id);
        }

        Ok(record_map)
    }

    async fn update_dns_records(&self, ip_address: &Ipv4Addr) -> anyhow::Result<Vec<DnsRecord>> {
        let zone_id = self.zone_name_to_id(&self.params.zone_name).await?;
        debug!("zone {0} (id: {1}) found", &self.params.zone_name, &zone_id);

        let record_map = self.record_names_to_ids(&zone_id).await?;
        for (ref record_name, ref record_id) in &record_map {
            debug!(
                "DNS record {2} (id: {3}) found in {0} (id: {1})",
                self.params.zone_name, zone_id, record_name, record_id
            )
        }

        let mut results = vec![];

        for (ref record_name, ref record_id) in record_map {
            let url = format!(
                "{0}/zones/{1}/dns_records/{2}",
                Self::endpoint(),
                zone_id,
                record_id
            );
            let json = json!({
                "type": "A",
                "name": record_name,
                "content": ip_address,
                "ttl": 1, // = automatic
            });
            let res: UpdateDnsRecord = self
                .client
                .put(&url)
                .json(&json)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            results.push(res.result);
        }

        Ok(results)
    }

    async fn zone_name_to_id<S: AsRef<str>>(&self, zone_name: S) -> anyhow::Result<String> {
        let zone_name = zone_name.as_ref();
        let url = format!("{0}/zones", Self::endpoint());
        let res: Zones = self
            .client
            .get(&url)
            .query(&[("name", zone_name)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        match res.result.first() {
            Some(zone) => Ok(zone.id.clone()),
            None => bail!("zone {0} does not exist", zone_name),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{CFClient, CFClientParams};
    use mockito::{mock, Matcher};
    use serde_json::json;

    #[test]
    fn test_new() {
        let _client = CFClient::new(CFClientParams::default());
    }

    #[tokio::test]
    async fn test_zone_name_to_id() -> Result<(), anyhow::Error> {
        let body = serde_json::to_string(&json!({
            "result": [
                {
                    "id": "1a2b3c4d",
                    "name": "zone",
                }
            ]
        }))?;

        let _m = mock("GET", "/zones")
            .match_query(Matcher::UrlEncoded("name".into(), "zone".into()))
            .with_status(200)
            .with_body(body)
            .create();

        let client = CFClient::new(CFClientParams::default())?;
        let id = client.zone_name_to_id("zone").await?;
        assert_eq!(id, "1a2b3c4d");
        Ok(())
    }

    #[tokio::test]
    async fn test_zone_name_to_id_not_found() -> Result<(), anyhow::Error> {
        let body = serde_json::to_string(&json!({
            "result": []
        }))?;

        let _m = mock("GET", "/zones")
            .match_query(Matcher::UrlEncoded("name".into(), "zone".into()))
            .with_status(200)
            .with_body(body)
            .create();

        let client = CFClient::new(CFClientParams::default())?;
        assert!(client.zone_name_to_id("zone").await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_record_names_to_ids() -> Result<(), anyhow::Error> {
        let body = serde_json::to_string(&json!({
            "result": [
                {
                    "id": "2b3c4d5e",
                    "name": "www.example.com",
                    "content": "0.0.0.0"
                }
            ]
        }))?;

        let _m = mock("GET", "/zones/1a2b3c4d/dns_records")
            .match_query(Matcher::UrlEncoded("name".into(), "www.example.com".into()))
            .with_status(200)
            .with_body(body)
            .create();

        let client = CFClient::new(CFClientParams {
            record_names: vec!["www.example.com".into()],
            ..Default::default()
        })?;
        let ids = client.record_names_to_ids("1a2b3c4d").await?;
        assert_eq!(ids.get("www.example.com").unwrap(), "2b3c4d5e");
        Ok(())
    }

    #[tokio::test]
    async fn test_record_names_to_ids_not_found() -> Result<(), anyhow::Error> {
        let body = serde_json::to_string(&json!({
            "result": []
        }))?;

        let _m = mock("GET", "/zones/1a2b3c4d/dns_records")
            .match_query(Matcher::UrlEncoded("name".into(), "www.example.com".into()))
            .with_status(200)
            .with_body(body)
            .create();

        let client = CFClient::new(CFClientParams {
            record_names: vec!["www.example.com".into()],
            ..Default::default()
        })?;
        assert!(client.record_names_to_ids("1a2b3c4d").await.is_err());
        Ok(())
    }
}
