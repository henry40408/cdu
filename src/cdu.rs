use std::sync::{Arc, Mutex};
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
use ttl_cache::TtlCache;

use crate::{Opts, PublicIPError};

const HTTP_TIMEOUT: u64 = 30;

pub struct Cdu {
    opts: Opts,
    cache: Arc<Mutex<TtlCache<String, String>>>,
}

impl Cdu {
    pub fn new(opts: Opts) -> Self {
        let capacity = opts.record_name_list().len();
        Self {
            opts,
            // zone identifier and record identifiers
            cache: Arc::new(Mutex::new(TtlCache::new(capacity + 1))),
        }
    }

    pub fn cache_ttl(&self) -> Option<Duration> {
        if self.opts.cache_seconds > 0 {
            Some(Duration::from_secs(self.opts.cache_seconds))
        } else {
            None
        }
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

        let zone_id = match self.cache.lock().unwrap().get(&self.opts.zone) {
            Some(id) => {
                debug!("zone found in cache: {} ({})", &self.opts.zone, &id);
                id.clone()
            }
            None => {
                let params = ListZones {
                    params: ListZonesParams {
                        name: Some(self.opts.zone.clone()),
                        ..Default::default()
                    },
                };
                let res: ApiSuccess<Vec<Zone>> = client.request(&params).await?;
                let id = match res.result.first() {
                    Some(zone) => zone.id.to_string(),
                    None => bail!("zone not found: {}", self.opts.zone),
                };
                if let Some(ttl) = self.cache_ttl() {
                    let mut cache = self.cache.lock().unwrap();
                    cache.insert(self.opts.zone.clone(), id.clone(), ttl);
                }
                debug!(
                    "zone fetched from Cloudflare: {} ({})",
                    &self.opts.zone, &id
                );
                id
            }
        };

        let mut tasks = vec![];
        for record_name in self.opts.record_name_list() {
            let client = client.clone();
            let zone_id = zone_id.clone();
            let cache = self.cache.clone();
            let cache_ttl = self.cache_ttl();
            tasks.push(tokio::spawn(async move {
                if let Some(id) = cache.lock().unwrap().get(&record_name) {
                    debug!("record found in cache: {} ({})", &record_name, &id);
                    return Ok((id.clone(), record_name));
                }
                let params = ListDnsRecords {
                    zone_identifier: &zone_id,
                    params: ListDnsRecordsParams {
                        name: Some(record_name.clone()),
                        ..Default::default()
                    },
                };
                let res: ApiSuccess<Vec<DnsRecord>> = client.request(&params).await?;
                let id = match res.result.first() {
                    Some(dns_record) => dns_record.id.clone(),
                    None => bail!("DNS record not found: {}", record_name),
                };
                if let Some(ttl) = cache_ttl {
                    cache
                        .lock()
                        .unwrap()
                        .insert(record_name.clone(), id.clone(), ttl);
                }
                debug!("record fetched from Cloudflare: {} ({})", &record_name, &id);
                Ok((id, record_name))
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
