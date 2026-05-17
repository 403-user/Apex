use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub host: String,
    pub port: u16,
    pub use_ssl: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub timeout_secs: u64,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        ChannelConfig {
            host: "0.0.0.0".into(),
            port: 4444,
            use_ssl: false,
            cert_path: None,
            key_path: None,
            timeout_secs: 30,
        }
    }
}

pub enum Channel {
    Tcp(TcpChannel),
    Ssl(SslChannel),
}

pub struct TcpChannel {
    pub stream: Option<TcpStream>,
    pub listener: Option<TcpListener>,
    pub config: ChannelConfig,
    pub connected: bool,
}

pub struct SslChannel {
    pub stream: Option<tokio_native_tls::TlsStream<TcpStream>>,
    pub listener: Option<TcpListener>,
    pub config: ChannelConfig,
    pub connected: bool,
}

impl TcpChannel {
    pub fn bind(config: ChannelConfig) -> anyhow::Result<Self> {
        let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
        let listener = tokio::runtime::Runtime::new()?.block_on(TcpListener::bind(addr))?;
        tracing::info!("TCP bind listener on {}:{}", config.host, config.port);
        Ok(TcpChannel {
            stream: None,
            listener: Some(listener),
            config,
            connected: false,
        })
    }

    pub fn connect(config: ChannelConfig) -> anyhow::Result<Self> {
        let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
        let stream = tokio::runtime::Runtime::new()?.block_on(TcpStream::connect(addr))?;
        tracing::info!("TCP connected to {}:{}", config.host, config.port);
        Ok(TcpChannel {
            stream: Some(stream),
            listener: None,
            config,
            connected: true,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<()> {
        let listener = self.listener.as_ref().ok_or_else(|| anyhow::anyhow!("No listener"))?;
        let (stream, addr) = listener.accept().await?;
        tracing::info!("TCP accepted connection from {}", addr);
        self.stream = Some(stream);
        self.connected = true;
        Ok(())
    }

    pub async fn send(&mut self, data: &[u8]) -> anyhow::Result<usize> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        stream.write_all(data).await?;
        Ok(data.len())
    }

    pub async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        let n = stream.read(buf).await?;
        Ok(n)
    }

    pub fn close(&mut self) {
        self.stream = None;
        self.connected = false;
    }
}

impl SslChannel {
    pub fn bind(config: ChannelConfig) -> anyhow::Result<Self> {
        let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
        let listener = tokio::runtime::Runtime::new()?.block_on(TcpListener::bind(addr))?;
        tracing::info!("SSL bind listener on {}:{}", config.host, config.port);
        Ok(SslChannel {
            stream: None,
            listener: Some(listener),
            config,
            connected: false,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<()> {
        use tokio_native_tls::TlsAcceptor;
        let listener = self.listener.as_ref().ok_or_else(|| anyhow::anyhow!("No listener"))?;
        let (tcp, addr) = listener.accept().await?;
        tracing::info!("SSL connection from {}", addr);

        let identity = native_tls::Identity::from_pkcs12(&[], "")
            .map_err(|_| anyhow::anyhow!("No SSL identity configured: provide cert_path and key_path in ChannelConfig"))?;
        let acceptor = native_tls::TlsAcceptor::builder(identity).build()?;
        let acceptor = TlsAcceptor::from(acceptor);
        let stream = acceptor.accept(tcp).await?;
        self.stream = Some(stream);
        self.connected = true;
        Ok(())
    }

    pub async fn send(&mut self, data: &[u8]) -> anyhow::Result<usize> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        stream.write_all(data).await?;
        Ok(data.len())
    }

    pub async fn recv(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow::anyhow!("Not connected"))?;
        let n = stream.read(buf).await?;
        Ok(n)
    }

    pub fn close(&mut self) {
        self.stream = None;
        self.connected = false;
    }
}
