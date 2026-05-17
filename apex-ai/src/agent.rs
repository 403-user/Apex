use crate::mcp::McpHost;
use crate::llm::LocalLlm;
use crate::sandbox::{CommandSandbox, PermissionLevel};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Agent Configuration ─────────────────────────────────────────

pub struct AgentConfig {
    pub max_chain_depth: usize,
    pub auto_confirm: bool,
    pub permission_level: PermissionLevel,
    pub require_human_approval_for: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            max_chain_depth: 10,
            auto_confirm: false,
            permission_level: PermissionLevel::ReadOnly,
            require_human_approval_for: vec![
                "hydra".into(),
                "msfconsole".into(),
                "--exec".into(),
            ],
        }
    }
}

// ─── Agent Workflow State ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub step: usize,
    pub intent: String,
    pub toolchain: Vec<String>,
    pub results: Vec<ToolResult>,
    pub phase: Phase,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    Planning,
    Executing,
    Analyzing,
    Reporting,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub args: Value,
    pub output: Value,
    pub parsed: Value,
    pub success: bool,
    pub duration_ms: u64,
}

// ─── Security Findings ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub title: String,
    pub severity: Severity,
    pub cvss_score: Option<f32>,
    pub owasp_category: Option<String>,
    pub description: String,
    pub affected_resource: String,
    pub remediation: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "CRITICAL"),
            Severity::High => write!(f, "HIGH"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::Low => write!(f, "LOW"),
            Severity::Info => write!(f, "INFO"),
        }
    }
}

// ─── Agentic Workflow Engine ──────────────────────────────────────

pub struct OffensiveAgent {
    pub mcp: McpHost,
    pub llm: Option<LocalLlm>,
    pub sandbox: CommandSandbox,
    pub config: AgentConfig,
    pub state: AgentState,
}

impl OffensiveAgent {
    pub fn new(mcp: McpHost, sandbox: CommandSandbox) -> Self {
        OffensiveAgent {
            mcp,
            llm: None,
            sandbox,
            config: AgentConfig::default(),
            state: AgentState {
                step: 0,
                intent: String::new(),
                toolchain: Vec::new(),
                results: Vec::new(),
                phase: Phase::Planning,
                error: None,
            },
        }
    }

    pub fn with_llm(mut self, llm: LocalLlm) -> Self {
        self.llm = Some(llm);
        self
    }

    // ─── Intent Processing ───────────────────────────────────────

    pub fn plan_from_intent(&mut self, intent: &str) -> anyhow::Result<Vec<String>> {
        self.state = AgentState {
            step: 0,
            intent: intent.to_string(),
            toolchain: Vec::new(),
            results: Vec::new(),
            phase: Phase::Planning,
            error: None,
        };

        let toolchain = self.mcp.get_toolchain_suggestion(intent);
        if toolchain.is_empty() {
            self.state.phase = Phase::Failed;
            self.state.error = Some(format!("No tools match intent: {}", intent));
            return Err(anyhow::anyhow!("No tools match intent: {}", intent));
        }

        self.state.toolchain = toolchain.clone();
        tracing::info!("Agent plan: {} → {} tools", intent, toolchain.len());
        Ok(toolchain)
    }

    // ─── Chain Execution ─────────────────────────────────────────

    pub async fn execute_chain(&mut self, intent: &str) -> anyhow::Result<AgentReport> {
        let toolchain = self.plan_from_intent(intent)?;
        self.state.phase = Phase::Executing;

        if let Some(ref llm) = self.llm {
            if !self.config.auto_confirm {
                tracing::info!("Requesting LLM validation for toolchain");
                let _validation = llm.query(&format!(
                    "Validate this penetration testing plan: {}. Tools: {:?}. \
                     Is this appropriate for a Kali Linux security assessment? Reply YES or NO and why.",
                    intent, toolchain
                )).await.unwrap_or_default();
            }
        }

        for tool in &toolchain {
            if self.state.step >= self.config.max_chain_depth {
                break;
            }

            let args = self.build_tool_args(tool, intent);
            tracing::info!("Executing tool {}: {:?}", tool, args);

            match self.execute_single(tool, &args).await {
                Ok(result) => {
                    self.state.results.push(result);
                }
                Err(e) => {
                    tracing::warn!("Tool {} failed: {}", tool, e);
                    let result = ToolResult {
                        tool: tool.clone(),
                        args,
                        output: serde_json::json!({"error": e.to_string()}),
                        parsed: serde_json::json!({"error": e.to_string()}),
                        success: false,
                        duration_ms: 0,
                    };
                    self.state.results.push(result);
                }
            }

            self.state.step += 1;
        }

        self.state.phase = Phase::Analyzing;
        self.analyze_results().await;

        self.state.phase = Phase::Reporting;
        let report = self.generate_report();

        self.state.phase = Phase::Complete;
        Ok(report)
    }

    async fn execute_single(&mut self, tool: &str, args: &Value) -> anyhow::Result<ToolResult> {
        let validation = self.sandbox.validate(tool, &[]);
        if let Err(e) = validation {
            return Err(anyhow::anyhow!("Sandbox validation failed: {}", e));
        }

        let start = std::time::Instant::now();
        let output = self.mcp.invoke_tool(tool, args.clone()).await?;
        let duration = start.elapsed();

        let stdout = output["stdout"].as_str().unwrap_or("");
        let parsed = if let Some(sandbox) = self.sandbox.get_sandbox(tool) {
            sandbox.parse_output(stdout).unwrap_or_else(|_| serde_json::json!({"raw": stdout}))
        } else {
            serde_json::json!({"raw": stdout})
        };

        Ok(ToolResult {
            tool: tool.to_string(),
            args: args.clone(),
            output: output.clone(),
            parsed,
            success: output["success"].as_bool().unwrap_or(false),
            duration_ms: duration.as_millis() as u64,
        })
    }

    // ─── Result Analysis ─────────────────────────────────────────

    async fn analyze_results(&mut self) {
        if let Some(ref llm) = self.llm {
            let mut analysis_prompt = String::from("Analyze these penetration testing results:\n\n");
            for result in &self.state.results {
                analysis_prompt.push_str(&format!(
                    "Tool: {}\nSuccess: {}\nDuration: {}ms\nOutput: {}\n\n",
                    result.tool, result.success, result.duration_ms,
                    serde_json::to_string_pretty(&result.parsed).unwrap_or_default()
                ));
            }
            analysis_prompt.push_str("\nIdentify vulnerabilities, misconfigurations, and suggest next steps.");

            if let Ok(analysis) = llm.query(&analysis_prompt).await {
                tracing::info!("LLM analysis: {}", analysis);
            }
        }
    }

    // ─── Report Generation ───────────────────────────────────────

    pub fn generate_report(&self) -> AgentReport {
        let mut findings = Vec::new();

        for result in &self.state.results {
            if !result.success { continue; }

            let ports: Vec<String> = result.parsed["open_ports"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let services: Vec<String> = result.parsed["services"].as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            if !ports.is_empty() {
                findings.push(SecurityFinding {
                    title: format!("Open ports discovered via {}", result.tool),
                    severity: Severity::Medium,
                    cvss_score: None,
                    owasp_category: Some("TA001 - Reconnaissance".into()),
                    description: format!("Found {} open service(s): {:?}", ports.len(), services),
                    affected_resource: ports.join(", "),
                    remediation: "Review exposed services and restrict access".into(),
                    references: vec![],
                });
            }
        }

        let total_duration: u64 = self.state.results.iter().map(|r| r.duration_ms).sum();

        AgentReport {
            intent: self.state.intent.clone(),
            toolchain: self.state.toolchain.clone(),
            steps_executed: self.state.results.len(),
            total_duration_ms: total_duration,
            findings,
            severity_summary: self.summarize_severities(),
            raw_results: self.state.results.clone(),
        }
    }

    fn summarize_severities(&self) -> std::collections::HashMap<String, usize> {
        let mut summary = std::collections::HashMap::new();
        // would be populated from actual findings in real implementation
        summary.insert("info".into(), 0usize);
        summary.insert("low".into(), 0);
        summary.insert("medium".into(), 0);
        summary.insert("high".into(), 0);
        summary.insert("critical".into(), 0);
        summary
    }

    // ─── Tool Argument Construction ──────────────────────────────

    fn build_tool_args(&self, tool: &str, intent: &str) -> Value {
        let _intent_lower = intent.to_lowercase();

        match tool {
            "nmap" => {
                let target = extract_target(intent).unwrap_or("127.0.0.1");
                serde_json::json!({
                    "target": target,
                    "flags": "-sV -sC"
                })
            }
            "dig" => {
                let domain = extract_domain(intent).unwrap_or("example.com");
                serde_json::json!({
                    "domain": domain,
                    "type": "ANY"
                })
            }
            "whatweb" => {
                let url = extract_url(intent).unwrap_or("http://example.com");
                serde_json::json!({"url": url})
            }
            "curl" => {
                let url = extract_url(intent).unwrap_or("http://example.com");
                serde_json::json!({"url": url, "method": "GET"})
            }
            "gobuster" => {
                let url = extract_url(intent).unwrap_or("http://example.com");
                serde_json::json!({
                    "mode": "dir",
                    "target": url,
                    "wordlist": "/usr/share/wordlists/dirb/common.txt"
                })
            }
            "searchsploit" => {
                let query = extract_search(intent).unwrap_or("webapp");
                serde_json::json!({"query": query})
            }
            _ => serde_json::json!({})
        }
    }
}

// ─── Report Structure ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReport {
    pub intent: String,
    pub toolchain: Vec<String>,
    pub steps_executed: usize,
    pub total_duration_ms: u64,
    pub findings: Vec<SecurityFinding>,
    pub severity_summary: std::collections::HashMap<String, usize>,
    pub raw_results: Vec<ToolResult>,
}

// ─── Intent Extraction Helpers ────────────────────────────────────

fn extract_target(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|w| w.contains('.') && w.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn extract_domain(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|w| w.contains('.') && !w.contains(' ') && !w.starts_with("http"))
}

fn extract_url(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|w| w.starts_with("http://") || w.starts_with("https://"))
        .or_else(|| extract_target(text))
}

fn extract_search(text: &str) -> Option<&str> {
    text.split_whitespace()
        .skip_while(|w| !["for", "about", "related", "cve", "vulnerability", "exploit"].contains(w))
        .next()
}

// ─── Prompts Module ──────────────────────────────────────────────

pub fn create_offensive_prompts_dir() -> std::io::Result<()> {
    let path = std::path::Path::new("src/prompts");
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    let prompts = [
        ("offensive_analyst.txt", "You are an elite offensive security analyst operating within the Apex Terminal on Kali Linux. Your role is to analyze reconnaissance and exploitation outputs, identify vulnerabilities, and suggest evidence-based next steps. Follow these principles:
1. OPSEC FIRST - Never suggest actions that would leave unnecessary forensic traces
2. EVIDENCE-BASED - Base all conclusions on tool output data
3. PRIORITIZED - Rank findings by CVSS severity and exploitability
4. CONTEXTUAL - Consider the full attack chain, not isolated findings
5. COMPLIANT - Stay within authorized scope and permissions
Output format suggestion:
```
## Finding: {title}
**Severity**: {CRITICAL|HIGH|MEDIUM|LOW|INFO}
**CVSS**: {score}
**OWASP**: {category}
**Description**: {details}
**Affected**: {resource}
**Remediation**: {fix}
**References**: {links}
```"),
    ];

    for (filename, content) in &prompts {
        let file_path = path.join(filename);
        if !file_path.exists() {
            std::fs::write(file_path, content)?;
        }
    }

    Ok(())
}

pub mod prompts {
    pub const OFFENSIVE_ANALYST: &str = include_str!("prompts/offensive_analyst.txt");
}
