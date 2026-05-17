pub mod mcp;
pub mod llm;
pub mod sandbox;
pub mod agent;

pub use mcp::McpHost;
pub use llm::{LocalLlm, LlmConfig, LlmBackend, QuantizationFormat, HardwareInfo, ChatMessage, ChatRole, LlmHealth, SystemPrompts};
pub use sandbox::{CommandSandbox, PermissionLevel, SandboxError, ToolSandbox, OutputParser};
pub use agent::{OffensiveAgent, AgentConfig, AgentReport, SecurityFinding, Severity, Phase, AgentState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_registration() {
        let host = McpHost::new();
        assert!(host.tools.contains_key("nmap"));
        assert!(host.tools.contains_key("ffuf"));
        assert!(host.tools.contains_key("gobuster"));
        assert_eq!(host.tools.len(), 12);
    }

    #[test]
    fn test_mcp_toolchain_recon() {
        let host = McpHost::new();
        let chain = host.get_toolchain_suggestion("scan the target network for open ports");
        assert!(chain.contains(&"nmap".to_string()));
    }

    #[test]
    fn test_mcp_toolchain_web() {
        let host = McpHost::new();
        let chain = host.get_toolchain_suggestion("scan the web application");
        assert!(chain.contains(&"nikto".to_string()) || chain.contains(&"gobuster".to_string()));
    }

    #[test]
    fn test_mcp_toolchain_brute() {
        let host = McpHost::new();
        let chain = host.get_toolchain_suggestion("brute force the ssh password");
        assert!(chain.contains(&"hydra".to_string()));
    }

    #[test]
    fn test_mcp_json_rpc_request() {
        let req = mcp::JsonRpcRequest::new(
            serde_json::json!(1),
            "tools/list",
            None,
        );
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/list");
    }

    #[test]
    fn test_mcp_json_rpc_error() {
        let err = mcp::JsonRpcError::method_not_found("unknown_tool");
        assert_eq!(err.code, -32601);
    }

    #[test]
    fn test_mcp_tool_invocation_fails_for_unknown() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
        let mut host = McpHost::new();
            let result = host.invoke_tool("nonexistent", serde_json::json!({})).await;
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_mcp_context_defaults() {
        let ctx = mcp::McpContext::default();
        assert_eq!(ctx.max_tool_calls_per_chain, 20);
        assert!(ctx.allow_network);
        assert!(!ctx.allow_filesystem_write);
    }

    #[test]
    fn test_llm_config_defaults() {
        let config = LlmConfig::default();
        assert_eq!(config.backend, LlmBackend::Ollama);
        assert_eq!(config.model, "llama3.1:8b");
        assert_eq!(config.endpoint, "http://localhost:11434");
    }

    #[test]
    fn test_llm_quantization() {
        assert_eq!(QuantizationFormat::Q4KM.vram_gb_for_7b(), 4.0);
        assert!(QuantizationFormat::Q4KM.quality_pct() > 98.0);
        assert_eq!(QuantizationFormat::Q4KM.to_string(), "Q4_K_M");
    }

    #[test]
    fn test_hardware_detection() {
        let hw = HardwareInfo::detect();
        assert!(hw.total_ram_gb > 0.0);
        assert!(hw.is_viable_for_7b());
    }

    #[test]
    fn test_sandbox_default_tools() {
        let sandbox = CommandSandbox::new();
        assert!(sandbox.sandboxes.contains_key("nmap"));
        assert!(sandbox.sandboxes.contains_key("hydra"));
        assert_eq!(sandbox.sandboxes.len(), 11);
    }

    #[test]
    fn test_sandbox_validation_allows_nmap() {
        let sandbox = CommandSandbox::new();
        assert!(sandbox.validate("nmap", &[]).is_ok());
    }

    #[test]
    fn test_sandbox_blocks_unknown_command() {
        let sandbox = CommandSandbox::new();
        let result = sandbox.validate("rm", &["-rf".to_string(), "/".to_string()]);
        assert!(matches!(result, Err(SandboxError::CommandNotAllowed(_))));
    }

    #[test]
    fn test_sandbox_permission_denied() {
        let mut sandbox = CommandSandbox::new();
        sandbox.set_permission(PermissionLevel::ReadOnly);
        let result = sandbox.validate("hydra", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_network_denied() {
        let mut sandbox = CommandSandbox::new();
        sandbox.allow_network = false;
        let result = sandbox.validate("nmap", &[]);
        assert!(matches!(result, Err(SandboxError::NetworkDenied(_))));
    }

    #[test]
    fn test_sandbox_blocklisted_flags() {
        let sandbox = CommandSandbox::new();
        let result = sandbox.validate("nmap", &["--exec".to_string()]);
        assert!(matches!(result, Err(SandboxError::BlockedFlags(_))));
    }

    #[test]
    fn test_sandbox_permission_levels() {
        assert!(PermissionLevel::NetworkScan.allows_network());
        assert!(PermissionLevel::Full.allows_filesystem_write());
        assert!(!PermissionLevel::ReadOnly.allows_network());
    }

    #[test]
    fn test_agent_planning() {
        let mcp = McpHost::new();
        let sandbox = CommandSandbox::new();
        let mut agent = OffensiveAgent::new(mcp, sandbox);

        let chain = agent.plan_from_intent("scan the target network").unwrap();
        assert!(!chain.is_empty());
        assert_eq!(agent.state.phase, Phase::Planning);

        let chain = agent.plan_from_intent("bruteforce passwords").unwrap();
        assert!(chain.contains(&"hydra".to_string()));
    }

    #[test]
    fn test_agent_report_empty() {
        let mcp = McpHost::new();
        let sandbox = CommandSandbox::new();
        let agent = OffensiveAgent::new(mcp, sandbox);
        let report = agent.generate_report();
        assert_eq!(report.steps_executed, 0);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn test_security_finding_creation() {
        let finding = SecurityFinding {
            title: "Open SSH Port".into(),
            severity: Severity::High,
            cvss_score: Some(7.5),
            owasp_category: Some("TA001".into()),
            description: "Port 22 is open".into(),
            affected_resource: "10.0.0.1:22".into(),
            remediation: "Restrict access".into(),
            references: vec![],
        };
        assert_eq!(finding.severity.to_string(), "HIGH");
        assert_eq!(finding.cvss_score, Some(7.5));
    }

    #[test]
    fn test_parsers_nmap() {
        let output = "22/tcp open ssh\n80/tcp open http\nOS details: Linux 5.x";
        let parsed = crate::sandbox::ToolSandbox {
            name: "nmap".into(),
            aliases: vec![],
            min_permission: PermissionLevel::NetworkScan,
            arg_rules: vec![],
            max_args: 16,
            requires_network: true,
            generates_output: true,
            output_parser: Some(sandbox::OutputParser::Nmap),
        }.parse_output(output).unwrap();

        assert_eq!(parsed["open_ports"].as_array().unwrap().len(), 2);
        assert!(parsed["os_info"].as_str().unwrap_or("").contains("Linux"));
    }

    #[test]
    fn test_parsers_gobuster() {
        let output = "/admin (Status: 301)\n/login (Status: 200)\n";
        let parsed = crate::sandbox::ToolSandbox {
            name: "gobuster".into(),
            aliases: vec![],
            min_permission: PermissionLevel::NetworkScan,
            arg_rules: vec![],
            max_args: 16,
            requires_network: true,
            generates_output: true,
            output_parser: Some(sandbox::OutputParser::Gobuster),
        }.parse_output(output).unwrap();

        assert_eq!(parsed["found_paths"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_agent_state_management() {
        let mut state = AgentState {
            step: 0,
            intent: "test".into(),
            toolchain: vec!["nmap".into(), "curl".into()],
            results: Vec::new(),
            phase: Phase::Planning,
            error: None,
        };

        assert_eq!(state.phase, Phase::Planning);
        state.phase = Phase::Executing;
        assert_eq!(state.phase, Phase::Executing);
        state.step += 1;
        assert_eq!(state.step, 1);
    }

    #[test]
    fn test_quantization_display() {
        assert_eq!(format!("{}", QuantizationFormat::F16), "F16");
        assert_eq!(format!("{}", QuantizationFormat::Q8_0), "Q8_0");
    }

    #[test]
    fn test_prompts_dir_creation() {
        assert!(agent::create_offensive_prompts_dir().is_ok());
    }
}
