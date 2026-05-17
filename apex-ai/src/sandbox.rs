use std::collections::{HashSet, HashMap};
use serde::{Deserialize, Serialize};

// ─── Permission Levels ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PermissionLevel {
    ReadOnly,
    NetworkScan,
    NetworkFull,
    FilesystemRead,
    FilesystemWrite,
    Full,
}

impl PermissionLevel {
    pub fn allows_network(&self) -> bool {
        matches!(self, PermissionLevel::NetworkScan | PermissionLevel::NetworkFull | PermissionLevel::Full)
    }

    pub fn allows_network_full(&self) -> bool {
        matches!(self, PermissionLevel::NetworkFull | PermissionLevel::Full)
    }

    pub fn allows_filesystem_write(&self) -> bool {
        matches!(self, PermissionLevel::FilesystemWrite | PermissionLevel::Full)
    }

    pub fn allows_filesystem_read(&self) -> bool {
        matches!(self, PermissionLevel::FilesystemRead | PermissionLevel::FilesystemWrite | PermissionLevel::Full)
    }
}

// ─── Argument Validation Rules ─────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ArgRule {
    pub flag: String,
    pub description: String,
    pub action: ArgAction,
}

#[derive(Debug, Clone)]
pub enum ArgAction {
    Allow,
    Deny,
    RequireValue,
    Warn(String),
}

// ─── Tool Sandbox Definition ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ToolSandbox {
    pub name: String,
    pub aliases: Vec<String>,
    pub min_permission: PermissionLevel,
    pub arg_rules: Vec<ArgRule>,
    pub max_args: usize,
    pub requires_network: bool,
    pub generates_output: bool,
    pub output_parser: Option<OutputParser>,
}

#[derive(Debug, Clone)]
pub enum OutputParser {
    Nmap,
    Gobuster,
    Ffuf,
    Nikto,
    WhatWeb,
    Generic { delimiter: String },
}

impl ToolSandbox {
    pub fn parse_output(&self, stdout: &str) -> anyhow::Result<Value> {
        match &self.output_parser {
            Some(OutputParser::Nmap) => Ok(parse_nmap_output(stdout)),
            Some(OutputParser::Gobuster) => Ok(parse_gobuster_output(stdout)),
            Some(OutputParser::Ffuf) => Ok(parse_ffuf_output(stdout)),
            Some(OutputParser::Nikto) => Ok(parse_nikto_output(stdout)),
            Some(OutputParser::WhatWeb) => Ok(parse_whatweb_output(stdout)),
            Some(OutputParser::Generic { .. }) => {
                Ok(serde_json::json!({ "raw": stdout }))
            }
            None => Ok(serde_json::json!({ "raw": stdout })),
        }
    }
}

use serde_json::Value;

fn parse_nmap_output(output: &str) -> Value {
    let mut open_ports = Vec::new();
    let mut services = Vec::new();
    let mut os_info = None;

    for line in output.lines() {
        if line.contains("/tcp") || line.contains("/udp") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                open_ports.push(parts[0]);
                services.push(parts[2]);
            }
        }
        if line.contains("OS details") || line.contains("Aggressive OS guesses") {
            os_info = Some(line.to_string());
        }
    }

    serde_json::json!({
        "open_ports": open_ports,
        "services": services,
        "os_info": os_info,
        "port_count": open_ports.len(),
        "raw": output
    })
}

fn parse_gobuster_output(output: &str) -> Value {
    let mut entries = Vec::new();

    for line in output.lines() {
        if line.starts_with('/') && (line.contains("(Status:") || line.contains("Status:")) {
            entries.push(line.to_string());
        }
    }

    serde_json::json!({
        "found_paths": entries,
        "count": entries.len(),
        "raw": output
    })
}

fn parse_ffuf_output(output: &str) -> Value {
    let mut entries = Vec::new();

    for line in output.lines() {
        if line.contains("|") && line.contains("Status:") {
            entries.push(line.to_string());
        }
    }

    serde_json::json!({
        "matches": entries,
        "count": entries.len(),
        "raw": output
    })
}

fn parse_nikto_output(output: &str) -> Value {
    let mut findings = Vec::new();

    for line in output.lines() {
        if line.contains("+ ") && !line.contains("Target IP") && !line.contains("Host") {
            findings.push(line.trim().to_string());
        }
    }

    serde_json::json!({
        "findings": findings,
        "count": findings.len(),
        "raw": output
    })
}

fn parse_whatweb_output(output: &str) -> Value {
    let mut technologies = Vec::new();

    for line in output.lines() {
        if line.contains('[') && line.contains(']') {
            technologies.push(line.trim().to_string());
        }
    }

    serde_json::json!({
        "technologies": technologies,
        "count": technologies.len(),
        "raw": output
    })
}

// ─── Command Sandbox (Main Validator) ─────────────────────────────

pub struct CommandSandbox {
    pub sandboxes: HashMap<String, ToolSandbox>,
    pub blocklisted_flags: HashSet<String>,
    pub max_args: usize,
    pub default_permission: PermissionLevel,
    pub allow_network: bool,
    pub allow_filesystem_write: bool,
}

impl CommandSandbox {
    pub fn new() -> Self {
        let mut sandbox = CommandSandbox {
            sandboxes: HashMap::new(),
            blocklisted_flags: HashSet::from([
                "--exec".into(), "-e".into(),
                "--interactive".into(), "-i".into(),
                "--no-sandbox".into(),
                "--privileged".into(),
            ]),
            max_args: 64,
            default_permission: PermissionLevel::ReadOnly,
            allow_network: true,
            allow_filesystem_write: false,
        };
        sandbox.register_default_tools();
        sandbox
    }

    fn register_default_tools(&mut self) {
        let tools = vec![
            ToolSandbox {
                name: "nmap".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![
                    ArgRule { flag: "--script".into(), description: "NSE scripts".into(), action: ArgAction::Allow },
                    ArgRule { flag: "-sC".into(), description: "Default scripts".into(), action: ArgAction::Allow },
                    ArgRule { flag: "-sV".into(), description: "Version detection".into(), action: ArgAction::Allow },
                    ArgRule { flag: "-O".into(), description: "OS detection".into(), action: ArgAction::Allow },
                    ArgRule { flag: "--traceroute".into(), description: "Trace route".into(), action: ArgAction::Allow },
                ],
                max_args: 16,
                requires_network: true,
                generates_output: true,
                output_parser: Some(OutputParser::Nmap),
            },
            ToolSandbox {
                name: "gobuster".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 12,
                requires_network: true,
                generates_output: true,
                output_parser: Some(OutputParser::Gobuster),
            },
            ToolSandbox {
                name: "ffuf".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 16,
                requires_network: true,
                generates_output: true,
                output_parser: Some(OutputParser::Ffuf),
            },
            ToolSandbox {
                name: "nikto".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 16,
                requires_network: true,
                generates_output: true,
                output_parser: Some(OutputParser::Nikto),
            },
            ToolSandbox {
                name: "whatweb".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 8,
                requires_network: true,
                generates_output: true,
                output_parser: Some(OutputParser::WhatWeb),
            },
            ToolSandbox {
                name: "curl".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![
                    ArgRule { flag: "--data".into(), description: "POST data".into(), action: ArgAction::Allow },
                    ArgRule { flag: "-X".into(), description: "HTTP method".into(), action: ArgAction::Allow },
                ],
                max_args: 16,
                requires_network: true,
                generates_output: true,
                output_parser: None,
            },
            ToolSandbox {
                name: "dig".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 8,
                requires_network: true,
                generates_output: true,
                output_parser: None,
            },
            ToolSandbox {
                name: "hydra".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkFull,
                arg_rules: vec![],
                max_args: 16,
                requires_network: true,
                generates_output: true,
                output_parser: None,
            },
            ToolSandbox {
                name: "searchsploit".into(),
                aliases: vec![],
                min_permission: PermissionLevel::ReadOnly,
                arg_rules: vec![],
                max_args: 8,
                requires_network: false,
                generates_output: true,
                output_parser: None,
            },
            ToolSandbox {
                name: "smbmap".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 10,
                requires_network: true,
                generates_output: true,
                output_parser: None,
            },
            ToolSandbox {
                name: "enum4linux".into(),
                aliases: vec![],
                min_permission: PermissionLevel::NetworkScan,
                arg_rules: vec![],
                max_args: 8,
                requires_network: true,
                generates_output: true,
                output_parser: None,
            },
        ];

        for tool in tools {
            self.sandboxes.insert(tool.name.clone(), tool);
        }
    }

    // ─── Validation ──────────────────────────────────────────────

    pub fn validate(&self, command: &str, args: &[String]) -> Result<ValidationResult, SandboxError> {
        let tool = self.sandboxes.get(command)
            .ok_or_else(|| SandboxError::CommandNotAllowed(command.to_string()))?;

        if tool.requires_network && !self.allow_network {
            return Err(SandboxError::NetworkDenied(command.to_string()));
        }

        let permission = self.effective_permission();
        if (tool.min_permission as u8) > (permission as u8) {
            return Err(SandboxError::PermissionDenied {
                command: command.to_string(),
                required: tool.min_permission,
                current: permission,
            });
        }

        if args.len() > tool.max_args {
            return Err(SandboxError::TooManyArgs(args.len(), tool.max_args));
        }

        let mut denied_flags = Vec::new();
        let mut warnings = Vec::new();

        for arg in args {
            let arg_lower = arg.to_lowercase();
            if self.blocklisted_flags.contains(&arg_lower) {
                denied_flags.push(arg.clone());
                continue;
            }
            if let Some(rule) = tool.arg_rules.iter().find(|r| r.flag == arg_lower || r.flag == *arg) {
                match &rule.action {
                    ArgAction::Deny => denied_flags.push(arg.clone()),
                    ArgAction::Warn(msg) => warnings.push(format!("{}: {}", arg, msg)),
                    _ => {}
                }
            }
        }

        if !denied_flags.is_empty() {
            return Err(SandboxError::BlockedFlags(denied_flags));
        }

        Ok(ValidationResult {
            tool_name: command.to_string(),
            permission_used: permission,
            args_parsed: args.to_vec(),
            warnings,
        })
    }

    fn effective_permission(&self) -> PermissionLevel {
        match (self.allow_network, self.allow_filesystem_write) {
            (true, true) => PermissionLevel::Full,
            (true, false) => PermissionLevel::NetworkScan,
            (false, true) => PermissionLevel::FilesystemWrite,
            (false, false) => PermissionLevel::ReadOnly,
        }
    }

    pub fn get_sandbox(&self, command: &str) -> Option<&ToolSandbox> {
        self.sandboxes.get(command)
    }

    pub fn register_sandbox(&mut self, sandbox: ToolSandbox) {
        self.sandboxes.insert(sandbox.name.clone(), sandbox);
    }

    pub fn set_permission(&mut self, level: PermissionLevel) {
        match level {
            PermissionLevel::Full => {
                self.allow_network = true;
                self.allow_filesystem_write = true;
            }
            PermissionLevel::NetworkFull | PermissionLevel::NetworkScan => {
                self.allow_network = true;
                self.allow_filesystem_write = false;
            }
            PermissionLevel::FilesystemWrite | PermissionLevel::FilesystemRead => {
                self.allow_network = false;
                self.allow_filesystem_write = true;
            }
            PermissionLevel::ReadOnly => {
                self.allow_network = false;
                self.allow_filesystem_write = false;
            }
        }
    }
}

// ─── Validation Results ───────────────────────────────────────────

pub struct ValidationResult {
    pub tool_name: String,
    pub permission_used: PermissionLevel,
    pub args_parsed: Vec<String>,
    pub warnings: Vec<String>,
}

// ─── Error Types ───────────────────────────────────────────────────

#[derive(Debug)]
pub enum SandboxError {
    CommandNotAllowed(String),
    PermissionDenied { command: String, required: PermissionLevel, current: PermissionLevel },
    NetworkDenied(String),
    TooManyArgs(usize, usize),
    BlockedFlags(Vec<String>),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::CommandNotAllowed(cmd) => write!(f, "Command not allowed: {}", cmd),
            SandboxError::PermissionDenied { command, required, current } => {
                write!(f, "Permission denied for '{}': need {:?}, have {:?}", command, required, current)
            }
            SandboxError::NetworkDenied(cmd) => write!(f, "Network access denied for '{}'", cmd),
            SandboxError::TooManyArgs(got, max) => write!(f, "Too many arguments: {} (max {})", got, max),
            SandboxError::BlockedFlags(flags) => write!(f, "Blocked flags: {:?}", flags),
        }
    }
}
