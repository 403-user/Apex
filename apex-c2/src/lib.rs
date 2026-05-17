pub mod sliver;
pub mod havoc;
pub mod mythic;
pub mod empire;

pub use sliver::SliverConnector;
pub use havoc::HavocConnector;
pub use mythic::MythicConnector;
pub use empire::EmpireConnector;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct C2Agent {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub arch: String,
    pub transport: String,
    pub internal_ip: String,
    pub external_ip: String,
    pub process_name: String,
    pub process_id: u32,
    pub framework: C2Framework,
    pub last_seen: String,
    pub first_seen: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum C2Framework {
    Sliver,
    Havoc,
    Mythic,
    Empire,
}

impl std::fmt::Display for C2Framework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            C2Framework::Sliver => write!(f, "Sliver"),
            C2Framework::Havoc => write!(f, "Havoc"),
            C2Framework::Mythic => write!(f, "Mythic"),
            C2Framework::Empire => write!(f, "Empire"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct C2CommandResult {
    pub agent_id: String,
    pub framework: C2Framework,
    pub command: String,
    pub output: String,
    pub success: bool,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct C2Manager {
    pub sliver: Option<SliverConnector>,
    pub havoc: Option<HavocConnector>,
    pub mythic: Option<MythicConnector>,
    pub empire: Option<EmpireConnector>,
}

impl C2Manager {
    pub fn new() -> Self {
        C2Manager {
            sliver: None,
            havoc: None,
            mythic: None,
            empire: None,
        }
    }

    pub fn with_sliver(mut self, endpoint: &str) -> Self {
        self.sliver = Some(SliverConnector::new(endpoint));
        self
    }

    pub fn with_havoc(mut self, endpoint: &str, password: &str) -> Self {
        self.havoc = Some(HavocConnector::new(endpoint, password));
        self
    }

    pub fn with_mythic(mut self, endpoint: &str, api_key: &str) -> Self {
        self.mythic = Some(MythicConnector::new(endpoint, api_key));
        self
    }

    pub fn with_empire(mut self, endpoint: &str, username: &str, password: &str) -> Self {
        self.empire = Some(EmpireConnector::new(endpoint, username, password));
        self
    }

    pub async fn list_all_agents(&self) -> Vec<C2Agent> {
        let mut agents = Vec::new();

        if let Some(ref sliver) = self.sliver {
            if let Ok(sessions) = sliver.list_sessions().await {
                for s in sessions {
                    agents.push(C2Agent {
                        id: s.id,
                        name: s.name,
                        hostname: s.hostname.clone(),
                        username: s.username,
                        os: s.os,
                        arch: s.arch,
                        transport: s.transport,
                        internal_ip: s.remote_address.clone(),
                        external_ip: String::new(),
                        process_name: s.hostname.clone(),
                        process_id: s.pid,
                        framework: C2Framework::Sliver,
                        last_seen: s.last_checkin.clone(),
                        first_seen: s.last_checkin,
                        active: true,
                    });
                }
            }
        }

        if let Some(ref havoc) = self.havoc {
            if let Ok(demons) = havoc.list_demons().await {
                for d in demons {
                    agents.push(C2Agent {
                        id: d.id.to_string(),
                        name: d.name,
                        hostname: d.hostname,
                        username: d.username,
                        os: d.os,
                        arch: d.arch,
                        transport: d.transport,
                        internal_ip: d.internal_ip,
                        external_ip: d.external_ip,
                        process_name: d.process_name,
                        process_id: d.process_id,
                        framework: C2Framework::Havoc,
                        last_seen: d.last_seen,
                        first_seen: d.first_seen,
                        active: true,
                    });
                }
            }
        }

        if let Some(ref mythic) = self.mythic {
            if let Ok(callbacks) = mythic.list_callbacks().await {
                for c in callbacks {
                    agents.push(C2Agent {
                        id: c.id,
                        name: c.hostname.clone(),
                        hostname: c.hostname,
                        username: c.user,
                        os: c.os,
                        arch: c.arch,
                        transport: "http".into(),
                        internal_ip: c.ip.clone(),
                        external_ip: String::new(),
                        process_name: c.process_name,
                        process_id: c.process_id,
                        framework: C2Framework::Mythic,
                        last_seen: c.last_checkin,
                        first_seen: c.registered_at,
                        active: c.active,
                    });
                }
            }
        }

        if let Some(ref empire) = self.empire {
            if let Ok(agents_list) = empire.list_agents().await {
                for a in agents_list {
                    agents.push(C2Agent {
                        id: a.id,
                        name: a.name,
                        hostname: a.hostname,
                        username: a.username,
                        os: a.os,
                        arch: a.arch,
                        transport: a.transport,
                        internal_ip: a.internal_ip,
                        external_ip: a.external_ip,
                        process_name: a.process_name,
                        process_id: a.process_id,
                        framework: C2Framework::Empire,
                        last_seen: a.last_seen,
                        first_seen: a.first_seen,
                        active: true,
                    });
                }
            }
        }

        agents
    }

    pub fn connected_frameworks(&self) -> Vec<C2Framework> {
        let mut frameworks = Vec::new();
        if self.sliver.is_some() { frameworks.push(C2Framework::Sliver); }
        if self.havoc.is_some() { frameworks.push(C2Framework::Havoc); }
        if self.mythic.is_some() { frameworks.push(C2Framework::Mythic); }
        if self.empire.is_some() { frameworks.push(C2Framework::Empire); }
        frameworks
    }
}

impl Default for C2Manager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c2_manager_empty() {
        let mgr = C2Manager::new();
        assert!(mgr.connected_frameworks().is_empty());
    }

    #[test]
    fn test_c2_manager_with_frameworks() {
        let mgr = C2Manager::new()
            .with_sliver("localhost:31337")
            .with_havoc("https://10.10.10.1:40056", "pass")
            .with_mythic("https://10.10.10.2:7443", "key")
            .with_empire("https://10.10.10.3:1337", "admin", "pass");

        let frameworks = mgr.connected_frameworks();
        assert_eq!(frameworks.len(), 4);
        assert!(frameworks.contains(&C2Framework::Sliver));
        assert!(frameworks.contains(&C2Framework::Havoc));
        assert!(frameworks.contains(&C2Framework::Mythic));
        assert!(frameworks.contains(&C2Framework::Empire));
    }

    #[tokio::test]
    async fn test_c2_manager_list_agents() {
        let mgr = C2Manager::new()
            .with_sliver("localhost:31337")
            .with_havoc("https://10.10.10.1:40056", "pass");

        let agents = mgr.list_all_agents().await;
        // Stub connectors return errors, so no agents are listed without a live server
        assert_eq!(agents.len(), 0);
    }

    #[test]
    fn test_c2_framework_display() {
        assert_eq!(C2Framework::Sliver.to_string(), "Sliver");
        assert_eq!(C2Framework::Havoc.to_string(), "Havoc");
        assert_eq!(C2Framework::Mythic.to_string(), "Mythic");
        assert_eq!(C2Framework::Empire.to_string(), "Empire");
    }
}
