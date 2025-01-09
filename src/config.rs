use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use aho_corasick::{AhoCorasick, Input, FindIter};
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use tokio::sync::RwLock;
use url::Url;


#[derive(Debug)]
pub struct BlackList {
    manager: AhoCorasick,
    path: PathBuf,
    last_modified: SystemTime,
}

impl BlackList {
    pub fn is_match<'h, I>(&self, input: I) -> bool 
        where I: Into<Input<'h>>
    {
        self.manager.is_match(input)
    }

    pub fn find_iter<'a, 'h, I>(&'a self, input: I) -> FindIter<'a, 'h>
        where I: Into<Input<'h>>
    {
        self.manager.find_iter(input) 
    }

    pub fn is_changed(&self) -> Result<bool> {
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
            manager: AhoCorasick::new(&patterns)?,
            last_modified: fs::metadata(&path)?.modified()?,
            path,
        })
    }
}


pub type SharedConfig = Arc<RwLock<Config>>;


#[derive(Debug)]
pub struct Config {
    pub default: ServerConfig,
    pub servers: Vec<ServerConfig>,
    path: Option<PathBuf>,
    last_modified: SystemTime,
}

impl Config {
    pub fn new(blacklist: PathBuf, servers: Vec<ServerConfig>) -> Result<Self> {
        Ok(Config {
            default: ServerConfig {
                url: "tcp://127.0.0.1:8080".parse()?, 
                blacklist: BlackList::try_from(blacklist)?,
            },
            servers,
            path: None,
            last_modified: UNIX_EPOCH,
        })
    }

    pub fn is_changed(&self) -> Result<bool> {
        if let Some(ref path) = self.path {
            let updated = fs::metadata(path)?.modified()? > self.last_modified;
            for server in self.servers.iter() {
                if server.blacklist.is_changed()? {
                    return Ok(true);
                }
            }
            return Ok(updated);
        }
        Ok(false)
    }

    pub fn update(&mut self) -> Result<()> {
        if let Some(ref path) = self.path {
            *self = Self::try_from(path.clone())?; 
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        #[derive(Deserialize)]
        struct TempConfig {
            default: ServerConfig,
            servers: Vec<ServerConfig>,
        }

        let temp = TempConfig::deserialize(deserializer)?;
        Ok(Config {
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
