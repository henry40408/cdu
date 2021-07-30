#![forbid(unsafe_code)]

use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cloudflare::framework::response::ApiFailure;
use cron::Schedule;
use log::info;
use structopt::StructOpt;
use tokio_retry::strategy::{jitter, ExponentialBackoff};

use cdu::{Cdu, Opts, PublicIPError};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::from_args();
    let context = Arc::new(Cdu::new(opts));

    if env::var_os("RUST_LOG").is_none() {
        if context.is_debug() {
            env::set_var("RUST_LOG", "cdu=debug");
        } else {
            env::set_var("RUST_LOG", "cdu=info");
        }
    }

    pretty_env_logger::init();

    if context.is_daemon() {
        run_daemon(context).await?;
    } else {
        context.run().await?;
    }

    Ok(())
}

async fn run_daemon(context: Arc<Cdu>) -> anyhow::Result<()> {
    let schedule = Schedule::from_str(context.cron())?;

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
        let context = context.clone();
        tokio_retry::RetryIf::spawn(
            strategy,
            || context.run(),
            |e: &anyhow::Error| e.is::<ApiFailure>() || e.is::<PublicIPError>(),
        )
        .await?;

        let duration = Instant::now() - instant;
        info!("done in {}ms", duration.as_millis());
    }

    Ok(())
}
