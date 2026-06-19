pub struct LuaEngine;

impl LuaEngine {
    pub fn new() -> anyhow::Result<Self> {
        Ok(LuaEngine)
    }

    pub fn load_config(&self, path: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("Lua config engine not available - mlua feature not enabled (path: {path})"))
    }

    pub fn eval_string(&self, _code: &str) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("Lua engine not available - mlua feature not enabled"))
    }

    pub fn execute_config_block(&self, _config: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("Lua engine not available - mlua feature not enabled"))
    }
}
