use std::{net::SocketAddr, sync::Arc};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use anyhow::{Context, Result, bail};
use log::{debug, info, error};
use rand::{thread_rng, Rng};
use tokio::{
    fs::File, 
    io::{
        AsyncRead, AsyncWrite, 
        AsyncReadExt, AsyncWriteExt, 
        ReadHalf, WriteHalf, 
        split
    }, 
    net::{TcpStream, TcpListener},
};
use url::Url;


type BlackList = AhoCorasick;

trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

trait Pipe: AsyncReadWrite {
    async fn pipe_stream<T>(&mut self, stream: &mut T) -> Result<()>
        where T: AsyncReadWrite;
    async fn pipe<T, U>(reader: ReadHalf<T>, writer: WriteHalf<U>) -> Result<()>
        where T: AsyncRead + Unpin + Send, U: AsyncWrite + Unpin + Send;
}

impl Pipe for TcpStream {
    async fn pipe_stream<T>(&mut self, stream: &mut T) -> Result<()> 
        where T: AsyncReadWrite
    {
        let (local_reader, local_writer) = split(stream);
        let (remote_reader, remote_writer) = split(self);
        tokio::try_join!(
            Self::pipe(local_reader, remote_writer), 
            Self::pipe(remote_reader, local_writer)
        )?;
        Ok(())
    }

    async fn pipe<T, U>(mut reader: ReadHalf<T>, mut writer: WriteHalf<U>) -> Result<()> 
        where T: AsyncRead + Unpin + Send, U: AsyncWrite + Unpin + Send
    {
       let mut buffer = vec![0; 2048];
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


#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let blacklist = Arc::new(load_blacklist().await?);
    let addr: SocketAddr = "127.0.0.1:8881".parse()?;
    let listener = TcpListener::bind(&addr).await?;
    info!("Proxy has started at {addr}");
    let mut i = 0;
    loop {
        let blacklist = Arc::clone(&blacklist);
        let (local_stream, addr) = listener.accept().await?;
        debug!("Accepted new client id={i} addr={addr}");
        let mut client = Client::new(i, local_stream, blacklist);
        tokio::task::spawn(async move {
            if let Err(err) = client.handle().await {
                error!("Client {i}: {err}");
            }
            if let Err(err) = client.close().await {
                error!("Client {i}: {err}");
            }
        });
        i += 1;
    }
}


async fn load_blacklist() -> Result<BlackList> {
    let mut buffer = String::new();
    File::open("full_blacklist").await?.read_to_string(&mut buffer).await?;
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

    async fn handle(&mut self) -> Result<()> {
        // Read stream
        let mut buffer = vec![0; 2048];
        let n = self.local_stream.read(&mut buffer).await?;
        buffer.truncate(n);
        debug!("Client {}: read {n} bytes", self.id);
        debug!("Client {}: buffer\n{}", self.id, String::from_utf8_lossy(&buffer));
        let parts = self.split_buffer(&buffer, b"\r\n\r\n").await;
        if parts.is_empty() {
            bail!("Unsupported request");
        }
        // Parse lines
        let lines: Vec<&[u8]> = self.split_buffer(parts[0], b"\r\n").await;
        if lines.is_empty() {
            bail!("Unsupported request");
        }
        let request_line = lines[0];
        let parts: Vec<&[u8]> = request_line.split(|&b| b == b' ').collect();
        if parts.len() < 3 {
            bail!("Unsupported request");
        }
        // Open remote TCP stream
        if parts[0] == b"CONNECT" {
            self.handle_https(parts[1]).await?;
        } else {
            self.handle_http(parts[1], &buffer).await?;
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
        let mut tcp_stream = TcpStream::connect((host.clone(), port)).await?;
        debug!("Client {}: connected to remote stream {host}:{port}", self.id);
        // Respond OK
        self.local_stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
        debug!("Client {}: sent 200 OK", self.id);
        // Process stream
        if self.blacklist.is_match(&host) && port == 443 {
            self.fragment_stream(&mut tcp_stream).await?;
        } 
        self.local_stream.pipe_stream(&mut tcp_stream).await?;
        Ok(())
    }

    async fn split_buffer<'a, 'b>(&'a self, buffer: &'b [u8], delimiter: &'b [u8]) -> Vec<&'b [u8]> {
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

    async fn fragment_stream(&mut self, remote_stream: &mut TcpStream) -> Result<()> {
        let mut buffer = vec![0; 1024];
        let n = self.local_stream.read(&mut buffer).await?;
        buffer.truncate(n);
        // debug!("Client {}: fragment buffer\n{}", self.id, String::from_utf8_lossy(&buffer));
        let (_head, mut remaining_data) = buffer.split_at_checked(5)
            .context("Unsupported request")?;
        for mtch in self.blacklist.find_iter(remaining_data) {
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

