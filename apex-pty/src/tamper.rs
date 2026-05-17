use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TamperType {
    CreatedFile { path: String, original_data: Option<Vec<u8>> },
    ReplacedFile { path: String, original_data: Vec<u8> },
    CreatedDirectory { path: String },
    ModifiedPermissions { path: String, original_mode: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tamper {
    pub id: String,
    pub source: String,
    pub tamper_type: TamperType,
    pub uid: u32,
    pub timestamp: String,
    pub reverted: bool,
}

pub struct TamperTracker {
    pub tampers: HashMap<String, Tamper>,
    pub tracking_enabled: bool,
}

impl TamperTracker {
    pub fn new() -> Self {
        TamperTracker {
            tampers: HashMap::new(),
            tracking_enabled: true,
        }
    }

    pub fn track_created_file(&mut self, path: &str, source: &str, uid: u32) {
        if !self.tracking_enabled {
            return;
        }
        let tamper = Tamper {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.to_string(),
            tamper_type: TamperType::CreatedFile {
                path: path.to_string(),
                original_data: None,
            },
            uid,
            timestamp: chrono_now(),
            reverted: false,
        };
        tracing::info!("Tracking created file: {}", path);
        self.tampers.insert(tamper.id.clone(), tamper);
    }

    pub fn track_replaced_file(&mut self, path: &str, original_data: Vec<u8>, source: &str, uid: u32) {
        if !self.tracking_enabled {
            return;
        }
        let tamper = Tamper {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.to_string(),
            tamper_type: TamperType::ReplacedFile {
                path: path.to_string(),
                original_data,
            },
            uid,
            timestamp: chrono_now(),
            reverted: false,
        };
        tracing::info!("Tracking replaced file: {}", path);
        self.tampers.insert(tamper.id.clone(), tamper);
    }

    pub fn track_created_directory(&mut self, path: &str, source: &str, uid: u32) {
        if !self.tracking_enabled {
            return;
        }
        let tamper = Tamper {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.to_string(),
            tamper_type: TamperType::CreatedDirectory { path: path.to_string() },
            uid,
            timestamp: chrono_now(),
            reverted: false,
        };
        tracing::info!("Tracking created directory: {}", path);
        self.tampers.insert(tamper.id.clone(), tamper);
    }

    pub fn revert_tamper(&mut self, id: &str) -> anyhow::Result<()> {
        let tamper = self.tampers.get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Tamper not found: {}", id))?;

        if tamper.reverted {
            return Err(anyhow::anyhow!("Tamper already reverted"));
        }

        fn map_io_err(e: std::io::Error) -> anyhow::Error {
            anyhow::anyhow!("I/O error during revert: {}", e)
        }

        match &tamper.tamper_type {
            TamperType::CreatedFile { path, .. } => {
                tracing::info!("Removing created file: {}", path);
                std::fs::remove_file(path).map_err(map_io_err)?;
            }
            TamperType::ReplacedFile { path, original_data } => {
                tracing::info!("Restoring original file: {} ({} bytes)", path, original_data.len());
                std::fs::write(path, original_data).map_err(map_io_err)?;
            }
            TamperType::CreatedDirectory { path } => {
                tracing::info!("Removing created directory: {}", path);
                std::fs::remove_dir(path).map_err(map_io_err)?;
            }
            TamperType::ModifiedPermissions { path, original_mode } => {
                tracing::info!("Restoring permissions for: {} ({:o})", path, original_mode);
                let perm = std::fs::Permissions::from_mode(*original_mode);
                std::fs::set_permissions(path, perm).map_err(map_io_err)?;
            }
        }

        tamper.reverted = true;
        Ok(())
    }

    pub fn revert_all(&mut self) -> Vec<(String, bool)> {
        let ids: Vec<String> = self.tampers.keys().cloned().collect();
        let mut results = Vec::new();
        for id in ids {
            let label = self.tampers.get(&id).map(|t| t.source.clone()).unwrap_or_default();
            let success = self.revert_tamper(&id).is_ok();
            results.push((label, success));
        }
        results
    }

    pub fn list_active(&self) -> Vec<&Tamper> {
        self.tampers.values().filter(|t| !t.reverted).collect()
    }

    pub fn close(&self) -> Vec<&Tamper> {
        let active: Vec<&Tamper> = self.tampers.values().filter(|t| !t.reverted).collect();
        if !active.is_empty() {
            tracing::warn!("{} un-reverted tamper(s) remain", active.len());
            for tamper in &active {
                match &tamper.tamper_type {
                    TamperType::CreatedFile { path, .. } => {
                        tracing::warn!("  [created] {} (from {})", path, tamper.source);
                    }
                    TamperType::ReplacedFile { path, .. } => {
                        tracing::warn!("  [replaced] {} (from {}) - revertable", path, tamper.source);
                    }
                    TamperType::CreatedDirectory { path } => {
                        tracing::warn!("  [mkdir] {} (from {})", path, tamper.source);
                    }
                    TamperType::ModifiedPermissions { path, .. } => {
                        tracing::warn!("  [chmod] {} (from {})", path, tamper.source);
                    }
                }
            }
        }
        active
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
