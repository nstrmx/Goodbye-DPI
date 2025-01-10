use std::{ fs, ops::Deref, path::PathBuf, sync::Arc, time::{SystemTime, UNIX_EPOCH} };
use aho_corasick::AhoCorasick;
use anyhow::Result;
use log::debug;
use serde::{Deserialize, Deserializer};
use tokio::sync::RwLock;
use url::Url;
use crate::DnsResolver;


pub type SharedConfig = Arc<RwLock<Config>>;


#[derive(Debug)]
pub struct Config {
    pub buffer_size: usize,
    pub dns: DnsResolver,
    pub default: ServerConfig,
    pub servers: Vec<ServerConfig>,
    path: Option<PathBuf>,
    last_modified: SystemTime,
}

impl Config {
    pub fn new(blacklist: PathBuf, servers: Vec<ServerConfig>) -> Result<Self> {
        Ok(Config {
            buffer_size: 4096,
            dns: DnsResolver::default(),
            default: ServerConfig {
                url: "tcp://127.0.0.1:8080".parse()?, 
                blacklist: BlackList::try_from(blacklist)?,
            },
            servers,
            path: None,
            last_modified: UNIX_EPOCH,
        })
    }

    pub fn changed(&self) -> Result<bool> {
        if let Some(ref path) = self.path {
            let modified = fs::metadata(path)?.modified()? > self.last_modified;
            for server in self.servers.iter() {
                if server.blacklist.changed()? {
                    return Ok(true);
                }
            }
            return Ok(modified);
        }
        Ok(false)
    }

    pub fn update(&mut self) -> Result<()> {
        if let Some(ref path) = self.path {
            *self = Self::try_from(path.clone())?; 
        }
        debug!("Config: updated");
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        #[derive(Deserialize)]
        struct TempConfig {
            buffer_size: usize,
            dns: DnsResolver,
            default: ServerConfig,
            servers: Vec<ServerConfig>,
        }

        let temp = TempConfig::deserialize(deserializer)?;
        Ok(Config {
            buffer_size: temp.buffer_size,
            dns: temp.dns,
            default: temp.default,
            servers: temp.servers,
            path: None,
            last_modified: UNIX_EPOCH,
        })   
    }
}

impl TryFrom<PathBuf> for Config {
    type Error = anyhow::Error;

    fn try_from(value: PathBuf) -> std::result::Result<Self, Self::Error> {
        let buffer = fs::read_to_string(&value)?;
        let mut config: Config = serde_yaml::from_str(&buffer)?;
        config.last_modified = fs::metadata(&value)?.modified()?;
        config.path = Some(value);
        Ok(config)
    }
}


#[derive(Debug)]
pub struct ServerConfig {
    pub url: Url,
    pub blacklist: BlackList,
}

impl<'de> Deserialize<'de> for ServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        #[derive(Deserialize)]
        struct TempServerConfig {
            url: String,
            blacklist: PathBuf,
        }

        let temp = TempServerConfig::deserialize(deserializer)?;
        Ok(ServerConfig {
            url: temp.url.parse().map_err(serde::de::Error::custom)?,
            blacklist: BlackList::try_from(temp.blacklist)
                .map_err(serde::de::Error::custom)?,
        })
    }
}


#[derive(Debug)]
pub struct BlackList {
    handler: AhoCorasick,
    path: PathBuf,
    last_modified: SystemTime,
}

impl BlackList {
    pub fn changed(&self) -> Result<bool> {
        let current_modified = fs::metadata(&self.path)?.modified()?;
        Ok(current_modified > self.last_modified)
    }
}

impl TryFrom<PathBuf> for BlackList {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        let patterns: Vec<String> = fs::read_to_string(&path)?
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.starts_with("//"))
            .collect();
        Ok(Self {
            handler: AhoCorasick::new(&patterns)?,
            last_modified: fs::metadata(&path)?.modified()?,
            path,
        })
    }
}

impl Deref for BlackList {
    type Target = AhoCorasick;
    
    fn deref(&self) -> &Self::Target {
        &self.handler
    }
}
