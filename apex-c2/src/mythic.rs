use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MythicCallback {
    pub id: String,
    pub user: String,
    pub hostname: String,
    pub ip: String,
    pub os: String,
    pub arch: String,
    pub process_name: String,
    pub process_id: u32,
    pub description: String,
    pub registered_at: String,
    pub last_checkin: String,
    pub active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MythicTaskResult {
    pub task_id: String,
    pub callback_id: String,
    pub command: String,
    pub parameters: String,
    pub output: String,
    pub status: String,
    pub completed: bool,
}

#[derive(Debug)]
pub struct MythicConnector {
    pub graphql_endpoint: String,
    api_key: String,
    user_agent_id: Option<String>,
    connected: bool,
    client: reqwest::Client,
}

impl MythicConnector {
    pub fn new(endpoint: &str, api_key: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("ApexTerminal-MythicConnector/0.1")
            .build()
            .unwrap_or_default();

        MythicConnector {
            graphql_endpoint: format!("{}/graphql", endpoint.trim_end_matches('/')),
            api_key: api_key.to_string(),
            user_agent_id: None,
            connected: false,
            client,
        }
    }

    async fn graphql_query(&self, query: &str, variables: Value) -> Result<Value> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .client
            .post(&self.graphql_endpoint)
            .header("Apollo-Require-Preflight", "true")
            .json(&body)
            .send()
            .await
            .context("Mythic GraphQL request failed")?;

        let data: Value = response.json::<Value>().await.context("Failed to parse Mythic GraphQL response")?;

        if let Some(errors) = data.get("errors") {
            anyhow::bail!("Mythic GraphQL errors: {}", errors);
        }

        Ok(data["data"].clone())
    }

    pub async fn connect_graphql(&mut self) -> Result<()> {
        tracing::info!("Connecting to Mythic GraphQL at {}", self.graphql_endpoint);

        let query = r#"
            mutation Authenticate($apikey: String!) {
                tokenGenerate(input: { api_key: $apikey }) {
                    access_token
                    token_type
                }
            }
        "#;

        let result = self.graphql_query(query, serde_json::json!({
            "apikey": format!("{}:{}", self.api_key, "apikey")
        })).await?;

        tracing::info!("Mythic authentication response: {}", result);
        self.connected = true;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub async fn list_callbacks(&self) -> Result<Vec<MythicCallback>> {
        tracing::info!("Listing Mythic callbacks");

        let query = r#"
            query Callbacks {
                callback(where: {}) {
                    id
                    user
                    host
                    ip
                    os
                    arch
                    process_name
                    process_id
                    description
                    registered_at
                    last_checkin
                    active
                }
            }
        "#;

        let data = self.graphql_query(query, serde_json::json!({})).await
            .map_err(|e| anyhow::anyhow!("Mythic list_callbacks failed: {}", e))?;
        let callbacks: Vec<MythicCallback> = data["callback"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|c| MythicCallback {
                        id: c["id"].as_str().unwrap_or("").to_string(),
                        user: c["user"].as_str().unwrap_or("").to_string(),
                        hostname: c["host"].as_str().unwrap_or("").to_string(),
                        ip: c["ip"].as_str().unwrap_or("").to_string(),
                        os: c["os"].as_str().unwrap_or("").to_string(),
                        arch: c["arch"].as_str().unwrap_or("").to_string(),
                        process_name: c["process_name"].as_str().unwrap_or("").to_string(),
                        process_id: c["process_id"].as_u64().and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
                        description: c["description"].as_str().unwrap_or("").to_string(),
                        registered_at: c["registered_at"].as_str().unwrap_or("").to_string(),
                        last_checkin: c["last_checkin"].as_str().unwrap_or("").to_string(),
                        active: c["active"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(callbacks)
    }

    pub async fn create_task(&self, callback_id: &str, command: &str, params: &str) -> Result<String> {
        tracing::info!("Creating Mythic task for {}: {} {}", callback_id, command, params);

        let query = r#"
            mutation CreateTask($callback_id: String!, $command: String!, $params: String!) {
                createTask(input: {
                    callback_id: $callback_id,
                    command: $command,
                    params: $params
                }) {
                    id
                    status
                }
            }
        "#;

        let data = self.graphql_query(query, serde_json::json!({
            "callback_id": callback_id,
            "command": command,
            "params": params,
        })).await.map_err(|e| anyhow::anyhow!("Mythic create_task failed: {}", e))?;
        Ok(data["createTask"]["id"].as_str().unwrap_or("mock-task-id").to_string())
    }

    pub async fn get_task_result(&self, task_id: &str) -> Result<MythicTaskResult> {
        anyhow::bail!("Mythic get_task_result({}): not fully implemented - requires GraphQL task result query stubs", task_id);
    }

    pub async fn watch_callbacks(&self) -> Result<()> {
        anyhow::bail!("Mythic watch_callbacks: not implemented - requires WebSocket subscription stubs");
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mythic_connector_creation() {
        let c = MythicConnector::new("https://10.10.10.2:7443", "apikey_secret");
        assert_eq!(c.graphql_endpoint, "https://10.10.10.2:7443/graphql");
        assert!(!c.is_connected());
    }

    #[tokio::test]
    async fn test_mythic_list_callbacks_no_fallback() {
        let c = MythicConnector::new("https://10.10.10.2:7443", "test");
        let err = c.list_callbacks().await.unwrap_err();
        assert!(err.to_string().contains("failed"));
    }
}
