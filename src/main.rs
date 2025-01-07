use std::{
    fs, 
    net::SocketAddr, 
    path::PathBuf, 
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH}
};
use aho_corasick::{AhoCorasick, Input, FindIter};
use anyhow::{Context, Result, bail};
use clap::Parser;
use log::{debug, info, error};
use rand::{thread_rng, Rng};
use serde::{Deserialize, Deserializer};
use tokio::{
    io::{
        AsyncRead, AsyncWrite, 
        AsyncReadExt, AsyncWriteExt, 
        ReadHalf, WriteHalf, 
        split
    }, 
    net::{TcpStream, TcpListener},
    sync::RwLock,
};
use tokio_socks::tcp::Socks5Stream;
use url::Url;


const BUFFER_SIZE: usize = 4096;


#[derive(Debug, Clone)]
struct BlackList {
    manager: AhoCorasick,
    path: PathBuf,
    last_modified: SystemTime,
}

impl BlackList {
    fn is_match<'h, I>(&self, input: I) -> bool 
        where I: Into<Input<'h>>
    {
        self.manager.is_match(input)
    }

    fn find_iter<'a, 'h, I>(&'a self, input: I) -> FindIter<'a, 'h>
        where I: Into<Input<'h>>
    {
        self.manager.find_iter(input) 
    }

    fn is_changed(&self) -> Result<bool> {
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


#[derive(Parser)]
struct Args {
    /// Path to the configuration file
    #[clap(short, long)]
    config: Option<PathBuf>,
}


#[derive(Debug)]
struct Config {
    default: ServerConfig,
    servers: Vec<ServerConfig>,
    path: Option<PathBuf>,
    last_modified: SystemTime,
}

impl Config {
    fn new(blacklist: PathBuf, servers: Vec<ServerConfig>) -> Result<Self> {
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

    fn is_changed(&self) -> Result<bool> {
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

    fn update(&mut self) -> Result<()> {
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

#[derive(Debug, Clone)]
struct ServerConfig {
    url: Url,
    blacklist: BlackList,
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


fn split_buffer<'a>(buffer: &'a [u8], delimiter: &'a [u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    while let Some(pos) = buffer[start..]
        .windows(delimiter.len())
        .position(|window| window == delimiter) 
    {
        let end = start + pos;
        parts.push(&buffer[start..end]);
        start = end + delimiter.len();
    }
    if start < buffer.len() {
        parts.push(&buffer[start..]);
    }
    parts
}


struct Client {
    id: usize,
    local_stream: TcpStream,
    config: Arc<RwLock<Config>>,
}

impl Client {
    fn new(id: usize, local_stream: TcpStream, config: Arc<RwLock<Config>>) -> Self {
        Self {
            id,
            local_stream,
            config,
        }
    }

    async fn handle(&mut self) -> Result<()> {
        // Read stream
        let mut buffer = vec![0; BUFFER_SIZE];
        let n = self.local_stream.read(&mut buffer).await?;
        buffer.truncate(n);
        if n == 0 {
            return Ok(());
        }
        debug!("Client {}: read {n} bytes", self.id);
        debug!("Client {}: buffer\n{}", self.id, String::from_utf8_lossy(&buffer));
        let parts = split_buffer(&buffer, b"\r\n\r\n");
        if parts.is_empty() {
            bail!("Unsupported request");
        }
        // Parse lines
        let lines: Vec<&[u8]> = split_buffer(parts[0], b"\r\n");
        if lines.is_empty() {
            bail!("Unsupported request");
        }
        let request_line = lines[0];
        let parts: Vec<&[u8]> = request_line.split(|&b| b == b' ').collect();
        if parts.len() < 3 {
            bail!("Unsupported request");
        }
        // Open remote TCP stream
        match parts[0] {
            b"CONNECT" => self.handle_https(parts[1]).await?,
            _ => self.handle_http(parts[1], &buffer).await?,
        }
        Ok(())
    }

    async fn handle_http(&mut self, target: &[u8], buffer: &[u8]) -> Result<()> {
        let url = String::from_utf8_lossy(target).to_string();
        let parsed_url = Url::parse(&url)?;
        let host = parsed_url.host_str().context("Failed to parse host")?.to_string();
        match parsed_url.scheme() {
            "http" => {
                let port = parsed_url.port().unwrap_or(80);
                let addr = format!("{}:{}", host, port);
                let mut tcp_stream = TcpStream::connect(addr).await?;
                tcp_stream.write_all(buffer).await?;
                tcp_stream.flush().await?;
                self.local_stream.pipe_stream(&mut tcp_stream).await?
            },
            _ => bail!("Unsupported scheme"),
        };        
        Ok(())
    }

    async fn handle_https(&mut self, target: &[u8]) -> Result<()> {
        let target_parts: Vec<&[u8]> = target.split(|&b| b == b':').collect();
        if target_parts.len() != 2 {
            bail!("Unsupported request");
        }
        let host_part = target_parts[0];
        let port_part = target_parts[1];
        let host = String::from_utf8_lossy(host_part).to_string();
        let port: u16 = String::from_utf8_lossy(port_part).parse()?;    
        // Connect to remote stream
        let server_count = {self.config.read().await.servers.len()};
        for i in 0..server_count {
            // Config may change any time
            let server_url = if let Some(server) = self.config.read().await.servers.get(i) {
                if !server.blacklist.is_match(&host) {
                    continue;
                }
                server.url.clone()
            } else {
                break;
            };
            debug!("Client {}: server {} matched {host}", self.id, server_url);
            let proxy_addr: SocketAddr = format!("{}:{}",
                server_url.host().context(format!("Unsupported url {}", server_url))?,
                server_url.port().context(format!("Unsupported url {}", server_url))?
            ).parse()?;
            let target_addr = format!("{host}:{port}");
            match server_url.scheme() {
                "socks5" => {
                    let mut stream = Socks5Stream::connect(proxy_addr, target_addr).await?;
                    self.local_stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
                    debug!("Client {}: sent 200 OK", self.id);
                    self.local_stream.pipe_stream(&mut stream).await?;
                }
                "tcp" => {
                    let mut stream = TcpStream::connect(proxy_addr).await?;
                    stream.write_all(format!(
                        "CONNECT {} HTTP/1.1\r\nHost: {}\r\nProxy-Connection: keep-alive\r\n\r\n", 
                        host, host
                    ).as_bytes()).await?;
                    self.local_stream.pipe_stream(&mut stream).await?;
                }
                _ => bail!("Unsupported url {}", server_url)
            }
            return Ok(());
        }
        let mut tcp_stream = TcpStream::connect((host.clone(), port)).await?;
        debug!("Client {}: connected to remote stream {host}:{port}", self.id);
        // Respond OK
        self.local_stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
        debug!("Client {}: sent 200 OK", self.id);
        // Process stream
        if port == 443 {
            let config = self.config.clone();
            self.fragment_stream(&mut tcp_stream, &config.read().await.default.blacklist).await?;
        } 
        self.local_stream.pipe_stream(&mut tcp_stream).await?;
        Ok(())
    }

    
    async fn fragment_stream(&mut self, remote_stream: &mut TcpStream, blacklist: &BlackList) -> Result<()> {
        let mut buffer = vec![0; BUFFER_SIZE];
        let n = self.local_stream.read(&mut buffer).await?;
        buffer.truncate(n);
        debug!("Client {}: fragment read {n} bytes", self.id);
        // debug!("Client {}: fragment buffer\n{}", self.id, String::from_utf8_lossy(&buffer));
        let (_head, mut remaining_data) = buffer.split_at_checked(5)
            .context("Unsupported request")?;
        for mtch in blacklist.find_iter(remaining_data) {
            let w = mtch.end() - mtch.start();
            let mid = mtch.end() - w / 2;
            if let Some((first, last)) = &remaining_data.split_at_checked(mid) {
                let fragment = self.process_fragment(first);
                remaining_data = last;
                remote_stream.write_all(&fragment).await?;
                remote_stream.flush().await?;
            };
        } 
        let part = self.process_fragment(remaining_data);
        remote_stream.write_all(&part).await?;
        remote_stream.flush().await?;
        Ok(())
    }

    #[inline]
    fn process_fragment(&self, fragment: &[u8]) -> Vec<u8> {
        let mut part = Vec::with_capacity(5 + fragment.len());
        part.extend_from_slice(&[0x16, 0x03]); // Starting bytes for the fragment
        part.push(thread_rng().gen_range(0..=255)); // Random byte
        part.extend_from_slice(&(fragment.len() as u16).to_be_bytes()); // Length of the fragment
        part.extend_from_slice(fragment); // The actual data fragment
        part
    }
    
    async fn close(mut self) -> Result<()> {
        self.local_stream.shutdown().await?;
        debug!("Client {}: closed", self.id);
        Ok(())
    }
}


trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}


trait Pipe: AsyncReadWrite {
    async fn pipe_stream<T>(&mut self, other: &mut T) -> Result<()>
        where T: AsyncReadWrite;
    
    async fn pipe<T, U>(reader: ReadHalf<T>, writer: WriteHalf<U>) -> Result<()>
        where T: AsyncRead + Unpin + Send, U: AsyncWrite + Unpin + Send;
}

impl Pipe for TcpStream {
    async fn pipe_stream<T>(&mut self, other: &mut T) -> Result<()> 
        where T: AsyncReadWrite
    {
        let (this_reader, this_writer) = split(self);
        let (other_reader, other_writer) = split(other);
        tokio::try_join!(
            Self::pipe(this_reader, other_writer), 
            Self::pipe(other_reader, this_writer),
        )?;
        Ok(())
    }

    async fn pipe<T, U>(mut reader: ReadHalf<T>, mut writer: WriteHalf<U>) -> Result<()> 
        where T: AsyncRead + Unpin + Send, U: AsyncWrite + Unpin + Send
    {
        let mut buffer = vec![0; BUFFER_SIZE];
        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }    
            writer.write_all(&buffer[..n]).await?;
            writer.flush().await?;
        }
        Ok(())
    }
}
