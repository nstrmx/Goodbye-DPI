use std::net::SocketAddr;
use anyhow::{bail, Context, Result};
use log::debug;
use rand::{thread_rng, Rng};
use tokio::{
    io::{AsyncRead, AsyncWrite,  AsyncReadExt, AsyncWriteExt,  split}, 
    net::TcpStream,
};
use tokio_socks::tcp::Socks5Stream;
use url::Url;
use crate::SharedConfig;


pub struct Client {
    id: usize,
    local_stream: TcpStream,
    config: SharedConfig,
}

impl Client {
    pub fn new(id: usize, local_stream: TcpStream, config: SharedConfig) -> Self {
        Self {
            id,
            local_stream,
            config,
        }
    }

    pub async fn handle(&mut self) -> Result<()> {
        // Read stream
        let buffer_size = self.config.read().await.buffer_size;
        debug!("Client {}: buffer size {buffer_size}", self.id);
        let mut buffer = vec![0; buffer_size];
        let n = self.local_stream.read(&mut buffer).await?;
        if n == 0 {
            return Ok(());
        }
        buffer.truncate(n);
        debug!("Client {}: read {n} bytes", self.id);
        debug!("Client {}: buffer\n{}", self.id, String::from_utf8_lossy(&buffer));
        let parts = split_buffer(&buffer, b"\r\n\r\n");
        let first_part = parts.first().context("Unsupported request")?;
        // Parse lines
        let lines: Vec<&[u8]> = split_buffer(first_part, b"\r\n");
        let request_line = lines.first().context("Unsupported request")?;
        let parts: Vec<&[u8]> = request_line.split(|&b| b == b' ').collect();
        let mut parts = parts.iter();
        let method = parts.next().context("Unsupported request")?;
        let host = parts.next().context("Unsupported request")?;
        // Open remote TCP stream
        match method {
            &b"CONNECT" => self.handle_https(host).await?,
            _ => self.handle_http(host, &buffer).await?,
        }
        Ok(())
    }

    async fn handle_http(&mut self, target: &[u8], buffer: &[u8]) -> Result<()> {
        let url = Url::parse(&String::from_utf8_lossy(target))?;
        let host = url.host_str().context("Failed to parse host")?;
        match url.scheme() {
            "http" => {
                let port = url.port().unwrap_or(80);
                let ip_addr = { self.config.read().await.dns.lookup_ip(host).await? };
                let target_addr = format!("{}:{}", ip_addr, port);
                let mut tcp_stream = TcpStream::connect(target_addr).await?;
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
        let mut target_parts = target_parts.iter();
        let host_part = target_parts.next().context("Unsupported target")?;
        let port_part = target_parts.next().context("Unsupported target")?;
        let host = String::from_utf8_lossy(host_part);
        let port: u16 = String::from_utf8_lossy(port_part).parse()?;    
        let ip_addr = self.config.read().await.dns.lookup_ip(&host).await?;
        let target_addr = format!("{ip_addr}:{port}");
        debug!("Client {}: target addr {target_addr}", self.id);
        // Connect to remote stream
        let server_count = self.config.read().await.servers.len();
        for i in 0..server_count {
            // Config may change any time
            let server_url = {
                let config = self.config.read().await;
                if let Some(server) = config.servers.get(i) {
                    if !server.blacklist.is_match(&*host) {
                        continue;
                    }
                    server.url.clone()
                } else {
                    break;
                }
            };
            debug!("Client {}: server {} matched {host}", self.id, server_url);
            let proxy_addr: SocketAddr = format!("{}:{}",
                server_url.host().context(format!("Unsupported url {}", server_url))?,
                server_url.port().context(format!("Unsupported url {}", server_url))?
            ).parse()?;
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
                        "CONNECT {} HTTP/1.1\r\n\
                         Host: {}\r\n\
                         Proxy-Connection: keep-alive\r\n\r\n", 
                        host, host
                    ).as_bytes()).await?;
                    self.local_stream.pipe_stream(&mut stream).await?;
                }
                _ => bail!("Unsupported url {}", server_url)
            }
            return Ok(());
        }
        let mut tcp_stream = TcpStream::connect(target_addr).await?;
        debug!("Client {}: connected to remote stream {host}:{port}", self.id);
        // Respond OK
        self.local_stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
        debug!("Client {}: sent 200 OK", self.id);
        // Process stream
        if port == 443 {
            self.fragment_stream(&mut tcp_stream).await?;
        } 
        self.local_stream.pipe_stream(&mut tcp_stream).await?;
        Ok(())
    }

    
    async fn fragment_stream(&mut self, remote_stream: &mut TcpStream) -> Result<()> {
        let buffer_size = self.config.read().await.buffer_size;
        let mut buffer = vec![0; buffer_size];
        let n = self.local_stream.read(&mut buffer).await?;
        buffer.truncate(n);
        debug!("Client {}: fragment read {n} bytes", self.id);
        let (_head, mut remaining_data) = buffer.split_at_checked(5)
            .context("Unsupported request")?;
        let matches: Vec<aho_corasick::Match> = {
            self.config.read().await
                .default.blacklist
                .find_iter(remaining_data)
                .collect()
        };
        for mtch in matches {
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
    
    pub async fn close(mut self) -> Result<()> {
        self.local_stream.shutdown().await?;
        debug!("Client {}: closed", self.id);
        Ok(())
    }
}


trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}


trait Pipe: AsyncReadWrite {
    async fn pipe_stream<T>(&mut self, other: &mut T) -> Result<()>
        where T: AsyncReadWrite + Unpin;
}

impl Pipe for TcpStream {
    async fn pipe_stream<T>(&mut self, other: &mut T) -> Result<()> 
        where T: AsyncReadWrite + Unpin
    {
        let (mut this_reader, mut this_writer) = split(self);
        let (mut other_reader, mut other_writer) = split(other);
        tokio::try_join!(
            tokio::io::copy(&mut this_reader, &mut other_writer),
            tokio::io::copy(&mut other_reader, &mut this_writer),
        )?;
        Ok(())
    }
}


fn split_buffer<'a>(buffer: &'a [u8], delimiter: &[u8]) -> Vec<&'a [u8]> {
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
