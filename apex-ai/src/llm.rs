use reqwest::Client;
use std::time::Duration;

// ─── Backend Types ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LlmBackend {
    Ollama,
    LlamaCpp,
    OpenAI,   // for future cloud fallback
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuantizationFormat {
    Q4KM,  // GGUF Q4_K_M - 75% reduction, <1% quality loss
    Q5KM,  // GGUF Q5_K_M
    Q8_0,  // GGUF Q8_0
    F16,   // Full float16
}

impl QuantizationFormat {
    pub fn vram_gb_for_7b(&self) -> f32 {
        match self {
            QuantizationFormat::Q4KM => 4.0,
            QuantizationFormat::Q5KM => 5.5,
            QuantizationFormat::Q8_0 => 8.0,
            QuantizationFormat::F16 => 14.0,
        }
    }

    pub fn quality_pct(&self) -> f32 {
        match self {
            QuantizationFormat::Q4KM => 99.0,
            QuantizationFormat::Q5KM => 99.5,
            QuantizationFormat::Q8_0 => 99.9,
            QuantizationFormat::F16 => 100.0,
        }
    }
}

impl std::fmt::Display for QuantizationFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuantizationFormat::Q4KM => write!(f, "Q4_K_M"),
            QuantizationFormat::Q5KM => write!(f, "Q5_K_M"),
            QuantizationFormat::Q8_0 => write!(f, "Q8_0"),
            QuantizationFormat::F16 => write!(f, "F16"),
        }
    }
}

// ─── Hardware Detection ───────────────────────────────────────────

pub struct HardwareInfo {
    pub has_avx: bool,
    pub has_avx2: bool,
    pub has_cuda: bool,
    pub has_metal: bool,
    pub total_ram_gb: f32,
    pub vram_gb: f32,
}

impl HardwareInfo {
    pub fn detect() -> Self {
        let has_avx = std::fs::read_to_string("/proc/cpuinfo")
            .map(|s| s.contains("avx2") || s.contains("avx "))
            .unwrap_or(false);

        let has_cuda = std::process::Command::new("nvidia-smi")
            .output().map(|o| o.status.success()).unwrap_or(false);

        let has_metal = cfg!(target_os = "macos");

        let total_ram_gb = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|k| k.parse::<f32>().ok())
                    .map(|kb| kb / 1024.0 / 1024.0)
            })
            .unwrap_or(8.0);

        HardwareInfo {
            has_avx,
            has_avx2: has_avx,
            has_cuda,
            has_metal,
            total_ram_gb,
            vram_gb: 0.0,
        }
    }

    pub fn suggested_quantization(&self) -> QuantizationFormat {
        if self.vram_gb >= 14.0 || self.total_ram_gb >= 32.0 {
            QuantizationFormat::Q8_0
        } else if self.vram_gb >= 5.5 || self.total_ram_gb >= 16.0 {
            QuantizationFormat::Q5KM
        } else {
            QuantizationFormat::Q4KM
        }
    }

    pub fn is_viable_for_7b(&self) -> bool {
        self.total_ram_gb >= 4.0
    }
}

// ─── LLM Configuration ────────────────────────────────────────────

pub struct LlmConfig {
    pub backend: LlmBackend,
    pub model: String,
    pub endpoint: String,
    pub quantization: QuantizationFormat,
    pub context_length: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
    pub timeout_secs: u64,
    pub num_gpu_layers: i32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        let hw = HardwareInfo::detect();
        LlmConfig {
            backend: LlmBackend::Ollama,
            model: "llama3.1:8b".into(),
            endpoint: "http://localhost:11434".into(),
            quantization: hw.suggested_quantization(),
            context_length: 4096,
            temperature: 0.1,
            top_p: 0.9,
            max_tokens: 2048,
            timeout_secs: 120,
            num_gpu_layers: if hw.has_cuda { 35 } else { 0 },
        }
    }
}

// ─── Chat Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for ChatRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatRole::System => write!(f, "system"),
            ChatRole::User => write!(f, "user"),
            ChatRole::Assistant => write!(f, "assistant"),
            ChatRole::Tool => write!(f, "tool"),
        }
    }
}

// ─── System Prompts ───────────────────────────────────────────────

pub struct SystemPrompts;

impl SystemPrompts {
    pub fn offensive_security_analyst() -> String {
        include_str!("prompts/offensive_analyst.txt").to_string()
    }

    pub fn default() -> String {
        "You are Apex, an AI security analysis assistant integrated into a next-generation \
         Kali Linux terminal. You have access to penetration testing tools via MCP. \
         Analyze output, suggest next steps, and explain findings clearly. \
         Always prioritize OPSEC and data privacy. When you identify vulnerabilities, \
         include CVSS severity ratings and OWASP classifications where applicable."
        .into()
    }
}

// ─── Ollama / Llama.cpp Client ────────────────────────────────────

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub struct LocalLlm {
    pub config: LlmConfig,
    client: Client,
    pub info: HardwareInfo,
}

impl LocalLlm {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_default();

        LocalLlm {
            info: HardwareInfo::detect(),
            config,
            client,
        }
    }

    pub fn new_ollama(model: &str, endpoint: &str) -> Self {
        let mut config = LlmConfig::default();
        config.backend = LlmBackend::Ollama;
        config.model = model.to_string();
        config.endpoint = endpoint.to_string();
        Self::new(config)
    }

    pub fn new_llamacpp(model: &str, endpoint: &str) -> Self {
        let mut config = LlmConfig::default();
        config.backend = LlmBackend::LlamaCpp;
        config.model = model.to_string();
        config.endpoint = endpoint.to_string();
        Self::new(config)
    }

    pub fn model_path_gguf(&self) -> String {
        format!("{}/{}-{}.gguf",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
            self.config.model.replace(':', "_"),
            self.config.quantization
        )
    }

    // ─── Ollama API ──────────────────────────────────────────────

    pub async fn ollama_generate(&self, prompt: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "top_p": self.config.top_p,
                "num_predict": self.config.max_tokens,
            }
        });

        let resp = self.client
            .post(&format!("{}/api/generate", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        Ok(result["response"].as_str().unwrap_or("").to_string())
    }

    pub async fn ollama_chat(&self, messages: &[ChatMessage]) -> anyhow::Result<String> {
        let msgs: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({
                "role": m.role.to_string(),
                "content": m.content
            })
        }).collect();

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "top_p": self.config.top_p,
                "num_predict": self.config.max_tokens,
            }
        });

        let resp = self.client
            .post(&format!("{}/api/chat", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        Ok(result["message"]["content"].as_str().unwrap_or("").to_string())
    }

    pub async fn ollama_stream(&self, prompt: &str, tx: mpsc::Sender<String>) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": prompt,
            "stream": true,
        });

        let mut stream = self.client
            .post(&format!("{}/api/generate", self.config.endpoint))
            .json(&body)
            .send()
            .await?
            .bytes_stream();

        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if let Ok(partial) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(response) = partial["response"].as_str() {
                        let _ = tx.send(response.to_string()).await;
                    }
                }
            }
        }

        Ok(())
    }

    // ─── Llama.cpp Server API ────────────────────────────────────

    pub async fn llamacpp_completion(&self, prompt: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "prompt": prompt,
            "temperature": self.config.temperature,
            "top_p": self.config.top_p,
            "n_predict": self.config.max_tokens,
            "stop": ["\n\n", "Human:", "Assistant:"]
        });

        let resp = self.client
            .post(&format!("{}/completion", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        Ok(result["content"].as_str().unwrap_or("").to_string())
    }

    pub async fn llamacpp_chat(&self, messages: &[ChatMessage]) -> anyhow::Result<String> {
        let msgs: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({
                "role": m.role.to_string(),
                "content": m.content
            })
        }).collect();

        let body = serde_json::json!({
            "messages": msgs,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
        });

        let resp = self.client
            .post(&format!("{}/v1/chat/completions", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        Ok(result["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
    }

    // ─── OpenAI API ──────────────────────────────────────────────

    pub async fn openai_chat(&self, messages: &[ChatMessage]) -> anyhow::Result<String> {
        let msgs: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({
                "role": m.role.to_string(),
                "content": m.content
            })
        }).collect();

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
        });

        let resp = self.client
            .post(&format!("{}/v1/chat/completions", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, body_text);
        }

        let result: serde_json::Value = resp.json().await?;
        let content = result["choices"][0]["message"]["content"].as_str()
            .ok_or_else(|| anyhow::anyhow!("OpenAI response missing choices[0].message.content: {}", result))?;
        Ok(content.to_string())
    }

    // ─── Unified API ─────────────────────────────────────────────

    pub async fn query(&self, prompt: &str) -> anyhow::Result<String> {
        match self.config.backend {
            LlmBackend::Ollama => self.ollama_generate(prompt).await,
            LlmBackend::LlamaCpp => self.llamacpp_completion(prompt).await,
            LlmBackend::OpenAI => {
                let msg = ChatMessage {
                    role: ChatRole::User,
                    content: prompt.to_string(),
                };
                self.openai_chat(&[msg]).await
            }
        }
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> anyhow::Result<String> {
        match self.config.backend {
            LlmBackend::Ollama => self.ollama_chat(messages).await,
            LlmBackend::LlamaCpp => self.llamacpp_chat(messages).await,
            LlmBackend::OpenAI => self.openai_chat(messages).await,
        }
    }

    // ─── Health Check ────────────────────────────────────────────

    pub async fn health_check(&self) -> anyhow::Result<LlmHealth> {
        let start = std::time::Instant::now();

        match self.config.backend {
            LlmBackend::Ollama => {
                let resp = self.client
                    .get(&format!("{}/api/tags", self.config.endpoint))
                    .send()
                    .await?;
                let elapsed = start.elapsed();
                if resp.status().is_success() {
                    let body: serde_json::Value = resp.json().await?;
                    let models: Vec<String> = body["models"].as_array()
                        .map(|arr| arr.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    Ok(LlmHealth {
                        reachable: true,
                        response_time_ms: elapsed.as_millis() as u64,
                        models,
                        ..Default::default()
                    })
                } else {
                    Ok(LlmHealth { reachable: false, ..Default::default() })
                }
            }
            LlmBackend::LlamaCpp => {
                let resp = self.client
                    .get(&format!("{}/health", self.config.endpoint))
                    .send()
                    .await?;
                let elapsed = start.elapsed();
                Ok(LlmHealth {
                    reachable: resp.status().is_success(),
                    response_time_ms: elapsed.as_millis() as u64,
                    ..Default::default()
                })
            }
            LlmBackend::OpenAI => {
                let resp = self.client
                    .get(&format!("{}/v1/models", self.config.endpoint))
                    .send()
                    .await?;
                let elapsed = start.elapsed();
                if resp.status().is_success() {
                    let body: serde_json::Value = resp.json().await?;
                    let models: Vec<String> = body["data"].as_array()
                        .map(|arr| arr.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    Ok(LlmHealth {
                        reachable: true,
                        response_time_ms: elapsed.as_millis() as u64,
                        models,
                        ..Default::default()
                    })
                } else {
                    Ok(LlmHealth {
                        reachable: false,
                        response_time_ms: elapsed.as_millis() as u64,
                        error: Some(format!("HTTP {}", resp.status())),
                        ..Default::default()
                    })
                }
            }
        }
    }
}

// ─── Health Status ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlmHealth {
    pub reachable: bool,
    pub response_time_ms: u64,
    pub models: Vec<String>,
    pub error: Option<String>,
}

impl Default for LlmHealth {
    fn default() -> Self {
        LlmHealth {
            reachable: false,
            response_time_ms: 0,
            models: Vec::new(),
            error: None,
        }
    }
}
