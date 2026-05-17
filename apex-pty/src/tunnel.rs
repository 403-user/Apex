use native_tls::{TlsConnector, TlsAcceptor, Identity};
use tokio::net::TcpStream;
use tokio_native_tls::{TlsConnector as TokioTlsConnector, TlsAcceptor as TokioTlsAcceptor};

pub struct TlsTunnel {
    identity: Option<Identity>,
    accept_invalid: bool,
}

impl TlsTunnel {
    pub fn new() -> Self {
        TlsTunnel {
            identity: None,
            accept_invalid: false,
        }
    }

    pub fn with_identity(mut self, der: &[u8], password: &str) -> anyhow::Result<Self> {
        let identity = Identity::from_pkcs12(der, password)?;
        self.identity = Some(identity);
        Ok(self)
    }

    pub fn with_invalid_certs(mut self) -> Self {
        self.accept_invalid = true;
        self
    }

    pub async fn connect(target: &str, port: u16) -> anyhow::Result<tokio_native_tls::TlsStream<TcpStream>> {
        let addr = format!("{}:{}", target, port);
        let stream = TcpStream::connect(&addr).await?;
        let connector = TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        let connector = TokioTlsConnector::from(connector);
        let tls = connector.connect(target, stream).await?;
        tracing::info!("TLS connected to {}:{}", target, port);
        Ok(tls)
    }

    pub async fn bind_listener(&self, port: u16) -> anyhow::Result<tokio::net::TcpListener> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("TLS bind listener on port {}", port);
        Ok(listener)
    }

    pub async fn accept(&self, listener: &tokio::net::TcpListener) -> anyhow::Result<tokio_native_tls::TlsStream<TcpStream>> {
        let identity = self.identity.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No TLS identity configured; call with_identity() first"))?;
        let (stream, peer) = listener.accept().await?;
        let acceptor = TlsAcceptor::builder(identity.clone()).build()?;
        let acceptor = TokioTlsAcceptor::from(acceptor);
        let tls = acceptor.accept(stream).await?;
        tracing::info!("TLS accept from {}", peer);
        Ok(tls)
    }
}
