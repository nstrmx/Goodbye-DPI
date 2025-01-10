use std::net::IpAddr;
use anyhow::{bail, Context, Result};
use rand::{thread_rng, Rng};
use serde::{ de::Error as DeError, Deserialize };
use trust_dns_resolver::{ config::{ResolverConfig, ResolverOpts}, TokioAsyncResolver as Resolver};


pub struct DnsResolver {
    resolver: Resolver,
}

impl DnsResolver {
    pub async fn lookup_ip(&self, host: &str) -> Result<IpAddr> {
        match self.resolver.lookup_ip(host).await {
            Ok(response) => {
                let addrs: Vec<IpAddr> = response.iter().collect();
                let rand_idx = thread_rng().gen_range(0..addrs.len());
                Ok(*addrs.get(rand_idx).context("Invalid host")?)
            }
            Err(err) => bail!(err),
        }
    }
}

impl std::fmt::Debug for DnsResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DnsResolver")
    }
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self{ resolver: Resolver::tokio(
            ResolverConfig::default(), ResolverOpts::default()
        )}
    }
}

impl<'de> Deserialize<'de> for DnsResolver {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
        where D: serde::Deserializer<'de> {
        #[derive(Deserialize)]
        struct TempDnsConfig {
            provider: String,
            protocol: String,
        }

        let temp = TempDnsConfig::deserialize(deserializer)?;
        let config = match (temp.provider.as_str(), temp.protocol.as_str()) {
            ("cloudflare", "default" | "tcp" | "udp") => ResolverConfig::cloudflare(),
            ("cloudflare", "https") => ResolverConfig::cloudflare_https(),
            ("cloudflare", "tls") => ResolverConfig::cloudflare_tls(),
            ("default", "default" | "tcp" | "udp") => ResolverConfig::default(),
            ("google", "default" | "tcp" | "udp") => ResolverConfig::google(),
            ("google", "https") => ResolverConfig::google_https(),
            ("google", "tls") => ResolverConfig::google_tls(),
            ("quad9", "default" | "tcp" | "udp") => ResolverConfig::quad9(),
            ("quad9", "https") => ResolverConfig::quad9_https(),
            ("quad9", "tls") => ResolverConfig::quad9_tls(),
            _ => return Err(DeError::custom(format!("Unsupported dns resolver config: {}/{}", temp.provider, temp.protocol))),
        };
        let resolver = Resolver::tokio(config, ResolverOpts::default());
        Ok(DnsResolver { resolver })
    }
}

