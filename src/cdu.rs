use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use cloudflare::endpoints::dns::{
    DnsContent, DnsRecord, ListDnsRecords, ListDnsRecordsParams, UpdateDnsRecord,
    UpdateDnsRecordParams,
};
use cloudflare::endpoints::zone::{ListZones, ListZonesParams, Zone};
use cloudflare::framework::async_api::{ApiClient, Client};
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::response::ApiSuccess;
use cloudflare::framework::{Environment, HttpApiClientConfig};
use log::debug;
use tokio::task::JoinHandle;

use crate::{Opts, PublicIPError};

const HTTP_TIMEOUT: u64 = 30;

pub struct Cdu {
    opts: Opts,
}

impl Cdu {
    pub fn new(opts: Opts) -> Self {
        Self { opts }
    }

    pub fn cron(&self) -> &str {
        &self.opts.cron
    }

    pub fn is_debug(&self) -> bool {
        self.opts.debug
    }

    pub fn is_daemon(&self) -> bool {
        self.opts.daemon
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let ip_address = public_ip::addr_v4().await.ok_or(PublicIPError)?;

        let credentials = Credentials::UserAuthToken {
            token: self.opts.token.clone(),
        };
        let config = HttpApiClientConfig {
            http_timeout: Duration::from_secs(HTTP_TIMEOUT),
            ..Default::default()
        };
        let client = Arc::new(Client::new(credentials, config, Environment::Production)?);

        debug!("public IPv4 address: {}", &ip_address);

        let params = ListZones {
            params: ListZonesParams {
                name: Some(self.opts.zone.clone()),
                ..Default::default()
            },
        };
        let res: ApiSuccess<Vec<Zone>> = client.request(&params).await?;
        let zone_id = match res.result.first() {
            Some(zone) => zone.id.to_string(),
            None => bail!("zone not found: {}", self.opts.zone),
        };

        debug!("zone found: {} ({})", &self.opts.zone, &zone_id);

        let mut tasks = vec![];
        for record_name in self.opts.record_name_list() {
            let client = client.clone();
            let zone_id = zone_id.clone();
            tasks.push(tokio::spawn(async move {
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
        for task in futures::future::join_all(tasks).await {
            let (dns_record_id, record_name) = task??;
            dns_record_ids.push((dns_record_id, record_name));
        }

        let mut tasks: Vec<JoinHandle<anyhow::Result<(String, String, String)>>> = vec![];
        for (dns_record_id, record_name) in dns_record_ids {
            let client = client.clone();
            let zone_id = zone_id.clone();
            tasks.push(tokio::spawn(async move {
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

                Ok((record_name, dns_record_id, content))
            }));
        }

        for task in futures::future::join_all(tasks).await {
            let (r, d, c) = task??;
            debug!("DNS record updated: {} ({}) -> {}", &r, &d, &c);
        }

        Ok(())
    }
}
