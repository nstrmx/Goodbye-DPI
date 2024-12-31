use std::{net::SocketAddr, sync::Arc};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use anyhow::{Context, Result, bail};
use log::{debug, info, error};
use rand::{self, Rng};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpStream, TcpListener}};


type BlackList = AhoCorasick;


#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let blacklist = Arc::new(load_blacklist().await?);
    const PORT: u16 = 8881;
    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
    let listener = TcpListener::bind(&addr).await?;
    info!("Proxy has started at 127.0.0.1:{}", PORT);
    let mut i = 0;
    loop {
        let blacklist = Arc::clone(&blacklist);
        let (local_stream, addr) = listener.accept().await?;
        debug!("Accepted new client id={i} addr={addr}");
        let client = Client::new(i, local_stream, blacklist);
        tokio::task::spawn(async move {
            if let Err(err) = client.handle().await {
                error!("Client {i}: {err}");
            }
        });
        i += 1;
    }
}


async fn load_blacklist() -> Result<BlackList> {
    let mut buffer = String::new();
    tokio::fs::File::open("full_blacklist").await?.read_to_string(&mut buffer).await?;
    let blacklist: Vec<String> = buffer.split('\n')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let blacklist = AhoCorasickBuilder::new().build(&blacklist)?;
    Ok(blacklist)
}


struct Client {
    id: usize,
    local_stream: TcpStream,
    blacklist: Arc<BlackList>,
}


impl Client {
    fn new(id: usize, local_stream: TcpStream, blacklist: Arc<BlackList>) -> Self {
        Self {
            id,
            local_stream,
            blacklist,
        }
    }

    async fn handle(mut self) -> Result<()> {
        // Read stream
        let mut http_buf = vec![0; 1024];
        let n = self.local_stream.read(&mut http_buf).await?;
        debug!("Client {}: read {n} bytes", self.id);
        http_buf.truncate(n);
        // Parse lines
        let lines: Vec<&[u8]> = http_buf.split(|&b| b == b'\r' || b == b'\n')
            .map(|p| p.trim_ascii_end())
            .collect();
        if lines.is_empty() {
            self.close().await?;
            bail!("Invalid request: {}", String::from_utf8_lossy(&http_buf));
        }
        let request_line = lines[0];
        let parts: Vec<&[u8]> = request_line.split(|&b| b == b' ').collect();
        if parts.len() < 3 || parts[0] != b"CONNECT" {
            self.close().await?;
            bail!("Invalid request: {}", String::from_utf8_lossy(&http_buf));
        }
        // Parse destination
        let target = parts[1];
        let target_parts: Vec<&[u8]> = target.split(|&b| b == b':').collect();
        if target_parts.len() != 2 {
            self.close().await?;
            bail!("Invalid request: {}", String::from_utf8_lossy(&http_buf));
        }
        let host_part = target_parts[0];
        let port_part = target_parts[1];
        let host = String::from_utf8_lossy(host_part).to_string();
        let port: u16 = String::from_utf8_lossy(port_part).parse()?;
        // Respond OK
        self.local_stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
        debug!("Client {}: sent 200 OK", self.id);
        // Connect to remote stream
        let remote_stream = TcpStream::connect((host.clone(), port)).await?;
        debug!("Client {}: connected to remote stream {host}:{port}", self.id);
        if port == 443 {
            self.fragment_stream(remote_stream).await?;
        } else {
            self.pipe(remote_stream).await?;
        }
        Ok(())
    }

    async fn close(mut self) -> Result<()> {
        self.local_stream.shutdown().await?;
        debug!("Client {}: closed", self.id);
        Ok(())
    }

    async fn pipe(mut self, mut remote_stream: TcpStream) -> Result<()> {
        let (mut local_reader, mut local_writer) = self.local_stream.split();
        let (mut remote_reader, mut remote_writer) = remote_stream.split();
        let local_to_remote = async {
            let mut buffer = vec![0; 2048];
            loop {
                let n = match local_reader.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                if let Err(err) = remote_writer.write_all(&buffer[..n]).await {
                    error!("Client {}: local_to_remote write: {err}", self.id);
                };
                if let Err(err) = remote_writer.flush().await {
                    error!("Client {}: local_to_remote flush: {err}", self.id);
                };
            }
        };
        let remote_to_local = async {
            let mut buffer = vec![0; 2048];
            loop {
                let n = match remote_reader.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                if let Err(err) = local_writer.write_all(&buffer[..n]).await {
                    error!("Client {}: remote_to_local write: {err}", self.id);
                };
                if let Err(err) = local_writer.flush().await {
                    error!("Client {}: remote_to_local flush: {err}", self.id);
                };
            }
        };
        tokio::join!(local_to_remote, remote_to_local);
        debug!("Client {}: pipe done", self.id);
        Ok(())
    }

    async fn fragment_stream(mut self, mut remote_stream: TcpStream) -> Result<()> {
        let mut data = vec![0; 2048];
        let n = self.local_stream.read(&mut data).await?;
        data.truncate(n);
        if n < 5 {
            return Ok(());
        }
        let mut remaining_data = &data[5..];
        for mtch in self.blacklist.find_iter(remaining_data) {
            let w = mtch.end() - mtch.start();
            let mid = mtch.end() - w / 2;
            let fragment = &remaining_data[..mid];
            let part = self.process_fragment(fragment);
            remote_stream.write_all(&part).await?;
            remote_stream.flush().await?;
            remaining_data = &remaining_data[mid..];
        } 
        let part = self.process_fragment(remaining_data);
        remote_stream.write_all(&part).await?;
        remote_stream.flush().await?;
        self.pipe(remote_stream).await?;
        Ok(())
    }

    fn process_fragment(&self, fragment: &[u8]) -> Vec<u8> {
        let mut part = vec![0x16, 0x03]; // Starting bytes for the fragment
        part.push(rand::thread_rng().gen_range(0..=255)); // Random byte
        part.extend_from_slice(&(fragment.len() as u16).to_be_bytes()); // Length of the fragment
        part.extend_from_slice(fragment); // The actual data fragment
        part
    }
}

