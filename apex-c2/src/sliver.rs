use anyhow::Result;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SliverSession {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub arch: String,
    pub transport: String,
    pub remote_address: String,
    pub pid: u32,
    pub last_checkin: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SliverImplantConfig {
    pub os: String,
    pub arch: String,
    pub format: String,
    pub name: String,
    pub is_shared: bool,
    pub is_service: bool,
    pub max_errors: u32,
    pub limit_domain: String,
    pub limit_hostname: String,
    pub limit_datetime: String,
    pub mtls_cn: String,
}

impl Default for SliverImplantConfig {
    fn default() -> Self {
        SliverImplantConfig {
            os: "linux".into(),
            arch: "amd64".into(),
            format: "EXECUTABLE".into(),
            name: "sliver-client".into(),
            is_shared: false,
            is_service: false,
            max_errors: 100,
            limit_domain: String::new(),
            limit_hostname: String::new(),
            limit_datetime: String::new(),
            mtls_cn: String::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SliverCommandResult {
    pub session_id: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub pid: u32,
}

#[derive(Debug)]
pub struct SliverConnector {
    pub grpc_endpoint: String,
    operator_cert: Option<String>,
    operator_key: Option<String>,
    ca_cert: Option<String>,
    pub mtu: usize,
    channel: Option<Channel>,
    connected: bool,
}

impl SliverConnector {
    pub fn new(endpoint: &str) -> Self {
        SliverConnector {
            grpc_endpoint: endpoint.to_string(),
            operator_cert: None,
            operator_key: None,
            ca_cert: None,
            mtu: 1500,
            channel: None,
            connected: false,
        }
    }

    pub fn with_tls(mut self, cert: &str, key: &str, ca: &str) -> Self {
        self.operator_cert = Some(cert.to_string());
        self.operator_key = Some(key.to_string());
        self.ca_cert = Some(ca.to_string());
        self
    }

    pub async fn connect(&mut self) -> Result<()> {
        tracing::info!("Connecting to Sliver gRPC at {}", self.grpc_endpoint);

        let scheme = if self.ca_cert.is_some() { "https" } else { "http" };
        let addr = format!("{}://{}", scheme, self.grpc_endpoint);

        let channel = if let Some(ca_pem) = &self.ca_cert {
            let ca = Certificate::from_pem(ca_pem);
            let tls = ClientTlsConfig::new()
                .ca_certificate(ca)
                .domain_name(&*self.grpc_endpoint.split(':').next().unwrap_or("localhost"));

            Channel::from_shared(addr)?
                .tls_config(tls)?
                .connect()
                .await?
        } else {
            Channel::from_shared(addr)?
                .connect()
                .await?
        };

        self.channel = Some(channel);
        self.connected = true;
        tracing::info!("Sliver gRPC connected successfully");
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub async fn list_sessions(&self) -> Result<Vec<SliverSession>> {
        anyhow::bail!("Sliver list_sessions: not implemented - requires Sliver gRPC client stubs in SliverRPCClient");
    }

    pub async fn interact_session(&self, session_id: &str) -> Result<SliverConnectorSession<'_>> {
        anyhow::bail!("Sliver interact_session({}): not implemented - requires Sliver gRPC client stubs", session_id);
    }

    pub async fn generate_implant(&self, config: &SliverImplantConfig) -> Result<Vec<u8>> {
        tracing::warn!("Sliver generate_implant({}/{}): not implemented - requires Sliver gRPC stubs", config.os, config.arch);
        anyhow::bail!("Sliver implant generation not implemented");
    }

    pub async fn close_session(&self, session_id: &str) -> Result<()> {
        anyhow::bail!("Sliver close_session({}): not implemented - requires Sliver gRPC stubs", session_id);
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        self.channel = None;
        Ok(())
    }
}

pub struct SliverConnectorSession<'a> {
    connector: &'a SliverConnector,
    session_id: String,
}

impl SliverConnectorSession<'_> {
    pub async fn execute(&self, command: &str) -> Result<SliverCommandResult> {
        anyhow::bail!("Sliver execute({} on {}): not implemented - requires Sliver gRPC stubs", command, self.session_id);
    }

    pub async fn upload(&self, remote_path: &str, data: Vec<u8>) -> Result<()> {
        anyhow::bail!("Sliver upload({}, {} bytes): not implemented - requires Sliver gRPC stubs", remote_path, data.len());
    }

    pub async fn download(&self, remote_path: &str) -> Result<Vec<u8>> {
        anyhow::bail!("Sliver download({}): not implemented - requires Sliver gRPC stubs", remote_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_connector_creation() {
        let c = SliverConnector::new("localhost:31337");
        assert_eq!(c.grpc_endpoint, "localhost:31337");
        assert!(!c.is_connected());
    }

    #[test]
    fn test_sliver_implant_config_defaults() {
        let cfg = SliverImplantConfig::default();
        assert_eq!(cfg.os, "linux");
        assert_eq!(cfg.arch, "amd64");
        assert_eq!(cfg.format, "EXECUTABLE");
    }
}
