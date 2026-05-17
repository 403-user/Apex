use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use tokio::io::AsyncReadExt;

// ─── JSON-RPC 2.0 Protocol Types ─────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn parse_error(msg: &str) -> Self {
        JsonRpcError { code: -32700, message: format!("Parse error: {}", msg), data: None }
    }
    pub fn invalid_request(msg: &str) -> Self {
        JsonRpcError { code: -32600, message: format!("Invalid request: {}", msg), data: None }
    }
    pub fn method_not_found(method: &str) -> Self {
        JsonRpcError { code: -32601, message: format!("Method not found: {}", method), data: None }
    }
    pub fn invalid_params(msg: &str) -> Self {
        JsonRpcError { code: -32602, message: format!("Invalid params: {}", msg), data: None }
    }
    pub fn internal_error(msg: &str) -> Self {
        JsonRpcError { code: -32603, message: format!("Internal error: {}", msg), data: None }
    }
    pub fn tool_execution_error(msg: &str, detail: &str) -> Self {
        JsonRpcError { code: -32000, message: msg.to_string(), data: Some(Value::String(detail.to_string())) }
    }
}

impl JsonRpcRequest {
    pub fn new(id: Value, method: &str, params: Option<Value>) -> Self {
        JsonRpcRequest { jsonrpc: "2.0".into(), id, method: method.into(), params }
    }

    pub fn notification(method: &str, params: Option<Value>) -> Self {
        JsonRpcRequest { jsonrpc: "2.0".into(), id: Value::Null, method: method.into(), params }
    }

    pub fn success_response(&self, result: Value) -> JsonRpcResponse {
        JsonRpcResponse { jsonrpc: "2.0".into(), id: self.id.clone(), result: Some(result), error: None }
    }

    pub fn error_response(&self, error: JsonRpcError) -> JsonRpcResponse {
        JsonRpcResponse { jsonrpc: "2.0".into(), id: self.id.clone(), result: None, error: Some(error) }
    }
}

// ─── MCP Tool Definitions ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_type: String,
    pub requires_network: bool,
    pub requires_filesystem: bool,
    pub timeout_seconds: u64,
}

impl ToolDefinition {
    pub fn new(name: &str, description: &str, schema: Value) -> Self {
        ToolDefinition {
            name: name.into(),
            description: description.into(),
            input_schema: schema,
            output_type: "text".into(),
            requires_network: false,
            requires_filesystem: false,
            timeout_seconds: 30,
        }
    }
}

// ─── MCP Transport Layer ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum McpTransport {
    Stdio { command: String, args: Vec<String> },
    Sse { url: String },
    WebSocket { url: String },
}

#[derive(Debug, Clone)]
pub struct McpServerConnection {
    pub id: Uuid,
    pub name: String,
    pub transport: McpTransport,
    pub tools: Vec<ToolDefinition>,
    pub status: ServerStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

// ─── MCP Host (Core Orchestrator) ──────────────────────────────────

pub struct McpHost {
    pub tools: HashMap<String, ToolDefinition>,
    pub servers: Vec<McpServerConnection>,
    pub context: McpContext,
    request_id: u64,
}

#[derive(Debug, Clone)]
pub struct McpContext {
    pub session_id: Uuid,
    pub tool_call_history: Vec<ToolCallRecord>,
    pub max_tool_calls_per_chain: usize,
    pub allow_network: bool,
    pub allow_filesystem_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool: String,
    pub args: Value,
    pub result: Value,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: String,
}

impl Default for McpContext {
    fn default() -> Self {
        McpContext {
            session_id: Uuid::new_v4(),
            tool_call_history: Vec::new(),
            max_tool_calls_per_chain: 20,
            allow_network: true,
            allow_filesystem_write: false,
        }
    }
}

impl McpHost {
    pub fn new() -> Self {
        let mut host = McpHost {
            tools: HashMap::new(),
            servers: Vec::new(),
            context: McpContext::default(),
            request_id: 1,
        };
        host.register_default_tools();
        host
    }

    fn register_default_tools(&mut self) {
        let kali_tools = vec![
            ToolDefinition::new("nmap", "Network discovery and security scanning", serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target IP or hostname"},
                    "ports": {"type": "string", "description": "Port range (e.g., 1-1000)"},
                    "flags": {"type": "string", "description": "Additional nmap flags (e.g., -sV -sC)"}
                },
                "required": ["target"]
            })),
            ToolDefinition::new("ffuf", "Fast web fuzzer for directory/file discovery", serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Target URL with FUZZ keyword"},
                    "wordlist": {"type": "string", "description": "Path to wordlist"},
                    "flags": {"type": "string", "description": "Additional ffuf flags"}
                },
                "required": ["url"]
            })),
            ToolDefinition::new("gobuster", "Directory/file & DNS busting tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["dir", "dns", "vhost"], "description": "Busting mode"},
                    "target": {"type": "string", "description": "Target URL or domain"},
                    "wordlist": {"type": "string", "description": "Path to wordlist"}
                },
                "required": ["mode", "target"]
            })),
            ToolDefinition::new("hydra", "Online password cracking tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target IP"},
                    "service": {"type": "string", "description": "Service (ssh, ftp, http-post-form, etc.)"},
                    "username": {"type": "string", "description": "Username or userlist"},
                    "password": {"type": "string", "description": "Password or password list"}
                },
                "required": ["target", "service"]
            })),
            ToolDefinition::new("searchsploit", "Exploit-DB search tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search term for vulnerabilities/exploits"}
                },
                "required": ["query"]
            })),
            ToolDefinition::new("curl", "HTTP request tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Target URL"},
                    "method": {"type": "string", "enum": ["GET", "POST", "PUT", "DELETE"]},
                    "headers": {"type": "string", "description": "Custom headers"},
                    "data": {"type": "string", "description": "Request body"}
                },
                "required": ["url"]
            })),
            ToolDefinition::new("dig", "DNS lookup utility", serde_json::json!({
                "type": "object",
                "properties": {
                    "domain": {"type": "string", "description": "Domain to query"},
                    "type": {"type": "string", "description": "Record type (A, AAAA, MX, TXT, etc.)"}
                },
                "required": ["domain"]
            })),
            ToolDefinition::new("msfconsole", "Metasploit Framework console", serde_json::json!({
                "type": "object",
                "properties": {
                    "resource": {"type": "string", "description": "Resource script to execute"},
                    "command": {"type": "string", "description": "Single command to run"}
                }
            })),
            ToolDefinition::new("nikto", "Web server scanner", serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target URL"},
                    "flags": {"type": "string", "description": "Additional nikto flags"}
                },
                "required": ["target"]
            })),
            ToolDefinition::new("whatweb", "Website fingerprinting tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Target URL"}
                },
                "required": ["url"]
            })),
            ToolDefinition::new("smbmap", "SMB enumeration tool", serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target IP or range"},
                    "share": {"type": "string", "description": "SMB share to enumerate"}
                },
                "required": ["target"]
            })),
            ToolDefinition::new("enum4linux", "Windows/Samba enumeration", serde_json::json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target IP"}
                },
                "required": ["target"]
            })),
        ];

        for tool in kali_tools {
            self.tools.insert(tool.name.clone(), tool);
        }
    }

    // ─── MCP Discovery Methods ─────────────────────────────────

    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.tools.values().cloned().collect()
    }

    pub fn register_tool(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get_tool(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    // ─── Server Management ────────────────────────────────────

    pub fn add_server(&mut self, name: &str, transport: McpTransport) -> Uuid {
        let id = Uuid::new_v4();
        self.servers.push(McpServerConnection {
            id,
            name: name.into(),
            transport,
            tools: Vec::new(),
            status: ServerStatus::Disconnected,
        });
        id
    }

    pub fn get_server(&self, id: &Uuid) -> Option<&McpServerConnection> {
        self.servers.iter().find(|s| s.id == *id)
    }

    pub fn remove_server(&mut self, id: &Uuid) {
        self.servers.retain(|s| s.id != *id);
    }

    // ─── Tool Invocation ─────────────────────────────────────────

    pub async fn invoke_tool(&mut self, tool_name: &str, args: Value) -> anyhow::Result<Value> {
        let tool = self.tools.get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not registered", tool_name))?;

        if !self.context.allow_network && tool.requires_network {
            return Err(anyhow::anyhow!("Network access denied for tool '{}'", tool_name));
        }
        if !self.context.allow_filesystem_write && tool.requires_filesystem {
            return Err(anyhow::anyhow!("Filesystem write denied for tool '{}'", tool_name));
        }

        let start = std::time::Instant::now();
        let result = self.execute_tool(tool_name, &args).await?;
        let duration = start.elapsed();

        let record = ToolCallRecord {
            tool: tool_name.into(),
            args: args.clone(),
            result: result.clone(),
            success: true,
            duration_ms: duration.as_millis() as u64,
            timestamp: chrono_now(),
        };
        self.context.tool_call_history.push(record);

        Ok(result)
    }

    async fn execute_tool(&self, tool_name: &str, args: &Value) -> anyhow::Result<Value> {
        match tool_name {
            "nmap" => self.exec_nmap(args).await,
            "curl" => self.exec_curl(args).await,
            "dig" => self.exec_dig(args).await,
            _ => {
                let cmd_str = format!("{} {}", tool_name, self.args_to_string(args));
                self.exec_shell(tool_name, &cmd_str).await
            }
        }
    }

    fn shell_quote(s: &str) -> String {
        let escaped = s.replace('\'', "'\\''");
        format!("'{}'", escaped)
    }

    async fn exec_capped(cmd: &mut tokio::process::Command) -> anyhow::Result<(String, String, Option<i32>)> {
        const MAX_OUTPUT: u64 = 1024 * 1024;
        let mut child = cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        if let Some(ref mut out) = child.stdout {
            out.take(MAX_OUTPUT).read_to_end(&mut stdout).await?;
        }
        if let Some(ref mut err) = child.stderr {
            err.take(MAX_OUTPUT).read_to_end(&mut stderr).await?;
        }

        let status = child.wait().await?;
        let exit_code = status.code();

        let stdout_str = String::from_utf8_lossy(&stdout).to_string();
        let stderr_str = String::from_utf8_lossy(&stderr).to_string();

        Ok((stdout_str, stderr_str, exit_code))
    }

    async fn exec_shell(&self, _tool: &str, shell_cmd: &str) -> anyhow::Result<Value> {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(shell_cmd);
        let (stdout, stderr, exit_code) = Self::exec_capped(&mut cmd).await?;
        let exit_code = exit_code.unwrap_or(-1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
            "success": true
        }))
    }

    async fn exec_nmap(&self, args: &Value) -> anyhow::Result<Value> {
        let target = args["target"].as_str().unwrap_or("");
        let ports = args["ports"].as_str().unwrap_or("");
        let flags = args["flags"].as_str().unwrap_or("-sV");

        let mut cmd = tokio::process::Command::new("nmap");
        for flag in flags.split_whitespace() {
            cmd.arg(flag);
        }
        cmd.arg(target);
        if !ports.is_empty() {
            cmd.arg("-p");
            cmd.arg(ports);
        }

        let (stdout, stderr, exit_code) = Self::exec_capped(&mut cmd).await?;
        let exit_code = exit_code.unwrap_or(-1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
            "success": true
        }))
    }

    async fn exec_curl(&self, args: &Value) -> anyhow::Result<Value> {
        let url = args["url"].as_str().unwrap_or("");
        let method = args["method"].as_str().unwrap_or("GET");
        let headers = args["headers"].as_str().unwrap_or("");

        let mut cmd = tokio::process::Command::new("curl");
        cmd.arg("-s");
        cmd.arg("-X");
        cmd.arg(method);
        for h in headers.split('\n') {
            let h = h.trim();
            if !h.is_empty() {
                cmd.arg("-H");
                cmd.arg(h);
            }
        }
        cmd.arg("--");
        cmd.arg(url);

        let (stdout, stderr, exit_code) = Self::exec_capped(&mut cmd).await?;
        let exit_code = exit_code.unwrap_or(-1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
            "success": true
        }))
    }

    async fn exec_dig(&self, args: &Value) -> anyhow::Result<Value> {
        let domain = args["domain"].as_str().unwrap_or("");
        let record_type = args["type"].as_str().unwrap_or("ANY");

        let mut cmd = tokio::process::Command::new("dig");
        cmd.arg(domain);
        cmd.arg(record_type);
        cmd.arg("+short");

        let (stdout, stderr, exit_code) = Self::exec_capped(&mut cmd).await?;
        let exit_code = exit_code.unwrap_or(-1);

        Ok(serde_json::json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
            "success": true
        }))
    }

    // ─── Toolchain Support ──────────────────────────────────────

    pub fn get_toolchain_suggestion(&self, intent: &str) -> Vec<String> {
        let intent_lower = intent.to_lowercase();
        let mut chain = Vec::new();

        if intent_lower.contains("recon") || intent_lower.contains("discovery") || intent_lower.contains("scan") {
            chain.push("nmap".into());
            chain.push("dig".into());
            chain.push("whatweb".into());
        }
        if intent_lower.contains("web") || intent_lower.contains("http") {
            chain.push("curl".into());
            chain.push("whatweb".into());
            chain.push("nikto".into());
            chain.push("gobuster".into());
        }
        if intent_lower.contains("smb") || intent_lower.contains("windows") {
            chain.push("smbmap".into());
            chain.push("enum4linux".into());
        }
        if intent_lower.contains("brute") || intent_lower.contains("password") || intent_lower.contains("crack") {
            chain.push("hydra".into());
        }
        if intent_lower.contains("vuln") || intent_lower.contains("cve") || intent_lower.contains("exploit") {
            chain.push("searchsploit".into());
            chain.push("nmap".into());
        }
        if intent_lower.contains("fuzz") {
            chain.push("ffuf".into());
            chain.push("gobuster".into());
        }

        chain.dedup();
        chain.retain(|t| self.tools.contains_key(t));
        chain
    }

    // ─── Utilities ──────────────────────────────────────────────

    fn args_to_string(&self, args: &Value) -> String {
        match args {
            Value::Object(map) => {
                map.iter().map(|(k, v)| {
                    let val = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => v.to_string(),
                    };
                    format!("--{} {}", k, Self::shell_quote(&val))
                }).collect::<Vec<_>>().join(" ")
            }
            Value::String(s) => Self::shell_quote(s),
            _ => Self::shell_quote(&args.to_string()),
        }
    }

    pub fn generate_request_id(&mut self) -> Value {
        let id = self.request_id;
        self.request_id += 1;
        Value::Number(serde_json::Number::from(id))
    }

    pub fn session_context(&self) -> &McpContext {
        &self.context
    }

    pub fn clear_history(&mut self) {
        self.context.tool_call_history.clear();
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
