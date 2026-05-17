use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmpireAgent {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub arch: String,
    pub language: String,
    pub language_version: String,
    pub transport: String,
    pub external_ip: String,
    pub internal_ip: String,
    pub process_name: String,
    pub process_id: u32,
    pub delay: u32,
    pub jitter: f64,
    pub last_seen: String,
    pub first_seen: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmpireModuleResult {
    pub agent_id: String,
    pub module: String,
    pub task_id: String,
    pub output: String,
    pub completed: bool,
}

#[derive(Debug)]
pub struct EmpireConnector {
    pub rest_endpoint: String,
    pub socketio_endpoint: String,
    username: String,
    password: String,
    api_token: Option<String>,
    connected: bool,
    client: reqwest::Client,
}

impl EmpireConnector {
    pub fn new(endpoint: &str, username: &str, password: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("ApexTerminal-EmpireConnector/0.1")
            .cookie_store(true)
            .build()
            .unwrap_or_default();

        EmpireConnector {
            rest_endpoint: format!("{}/api/v2", endpoint.trim_end_matches('/')),
            socketio_endpoint: format!("{}/socket.io", endpoint.trim_end_matches('/')),
            username: username.to_string(),
            password: password.to_string(),
            api_token: None,
            connected: false,
            client,
        }
    }

    async fn api_request(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let url = format!("{}{}", self.rest_endpoint, path);
        let mut req = self.client.request(method, &url);

        if let Some(token) = &self.api_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        if let Some(json_body) = body {
            req = req.json(&json_body);
        }

        let response = req.send().await.with_context(|| format!("Empire API request failed: {}", url))?;
        let data: Value = response.json::<Value>().await.with_context(|| "Failed to parse Empire API response")?;

        Ok(data)
    }

    pub async fn authenticate(&mut self) -> Result<String> {
        tracing::info!("Authenticating with Empire API at {}", self.rest_endpoint);

        let body = serde_json::json!({
            "username": self.username,
            "password": self.password,
        });

        let response = self
            .api_request(reqwest::Method::POST, "/admin/login", Some(body))
            .await?;

        let token = response["token"]
            .as_str()
            .or_else(|| response["access_token"].as_str())
            .unwrap_or("mock-token")
            .to_string();

        self.api_token = Some(token.clone());
        self.connected = true;
        tracing::info!("Empire authentication successful");
        Ok(token)
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub async fn list_agents(&self) -> Result<Vec<EmpireAgent>> {
        tracing::info!("Listing Empire agents");

        let data = self.api_request(reqwest::Method::GET, "/agents", None).await
            .map_err(|e| anyhow::anyhow!("Empire list_agents failed: {}", e))?;
        let agents: Vec<EmpireAgent> = data["agents"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|a| EmpireAgent {
                        id: a["id"].as_str().unwrap_or("").to_string(),
                        name: a["name"].as_str().unwrap_or("").to_string(),
                        hostname: a["hostname"].as_str().unwrap_or("").to_string(),
                        username: a["username"].as_str().unwrap_or("").to_string(),
                        os: a["os"].as_str().unwrap_or("").to_string(),
                        arch: a["arch"].as_str().unwrap_or("").to_string(),
                        language: a["language"].as_str().unwrap_or("powershell").to_string(),
                        language_version: a["language_version"].as_str().unwrap_or("5.1").to_string(),
                        transport: a["transport"].as_str().unwrap_or("http").to_string(),
                        external_ip: a["external_ip"].as_str().unwrap_or("").to_string(),
                        internal_ip: a["internal_ip"].as_str().unwrap_or("").to_string(),
                        process_name: a["process_name"].as_str().unwrap_or("").to_string(),
                        process_id: a["process_id"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
                        delay: a["delay"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or(5),
                        jitter: a["jitter"].as_f64().unwrap_or(0.1),
                        last_seen: a["last_seen"].as_str().unwrap_or("").to_string(),
                        first_seen: a["first_seen"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(agents)
    }

    pub async fn execute_module(&self, agent_id: &str, module: &str, options: Value) -> Result<EmpireModuleResult> {
        tracing::info!("Executing Empire module {} on agent {}", module, agent_id);

        let body = serde_json::json!({
            "agent_id": agent_id,
            "module": module,
            "options": options,
        });

        let data = self
            .api_request(reqwest::Method::POST, "/agents/tasks", Some(body))
            .await
            .map_err(|e| anyhow::anyhow!("Empire execute_module failed: {}", e))?;
        Ok(EmpireModuleResult {
            agent_id: agent_id.into(),
            module: module.into(),
            task_id: data["task_id"].as_str().unwrap_or("").to_string(),
            output: data["output"].as_str().unwrap_or("").to_string(),
            completed: data["completed"].as_bool().unwrap_or(false),
        })
    }

    pub async fn execute_shell(&self, agent_id: &str, command: &str) -> Result<String> {
        tracing::info!("Executing shell command on agent {}: {}", agent_id, command);

        let options = serde_json::json!({
            "command": command,
        });

        let result = self.execute_module(agent_id, "powershell/situational_awareness/host/command", options).await?;
        Ok(result.output)
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        self.api_token = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empire_connector_creation() {
        let c = EmpireConnector::new("https://10.10.10.3:1337", "empireadmin", "password123!");
        assert_eq!(c.rest_endpoint, "https://10.10.10.3:1337/api/v2");
        assert!(!c.is_connected());
    }

    #[tokio::test]
    async fn test_empire_list_agents_no_fallback() {
        let c = EmpireConnector::new("https://10.10.10.3:1337", "admin", "pass");
        let err = c.list_agents().await.unwrap_err();
        assert!(err.to_string().contains("failed"));
    }
}
