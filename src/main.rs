use std::{net::SocketAddr,  path::PathBuf,  sync::Arc};
use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, info, error};
use tokio::{net::TcpListener, sync::RwLock};

mod client;
use client::*;
mod config;
use config::*;
mod dns;
use dns::*;


#[derive(Parser)]
struct Args {
    /// Path to the configuration file
    #[clap(short, long)]
    config: Option<PathBuf>,
}


#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let config = if let Some(path) = args.config {
        Config::try_from(path)?
    } else {
        Config::new(PathBuf::from("default_blacklist"), vec![])?
    };
    // info!("Config: {config:?}");
    let url = &config.default.url;
    let addr: SocketAddr = (format!("{}:{}", 
        url.host().context("Unsupported url {url}")?, 
        url.port().context("Unsupported url {url}")?
    )).parse()?;
    let listener = TcpListener::bind(&addr).await?;
    let config = Arc::new(RwLock::new(config));
    info!("Proxy has started at {addr}");
    let mut i = 0;
    loop {
        let (local_stream, addr) = listener.accept().await?;
        debug!("Accepted new client id={i} addr={addr}");
        let mut client = Client::new(i, local_stream, Arc::clone(&config));
        tokio::task::spawn(async move {
            if let Err(err) = client.handle().await {
                error!("Client {i}: {err}");
            }
            if let Err(err) = client.close().await {
                error!("Client {i}: {err}");
            }
        });
        i += 1;
        if config.read().await.is_changed()? {
            config.write().await.update()?;     
        }
    }
}
