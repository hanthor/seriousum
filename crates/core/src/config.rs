//! Configuration types for seriousum.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub agent: AgentConfig,
    pub ebpf: EbpfConfig,
    pub network: NetworkConfig,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            ebpf: EbpfConfig::default(),
            network: NetworkConfig::default(),
            options: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub name: String,
    pub node_name: String,
    pub cluster_name: String,
    pub cluster_id: u64,
    pub enable_ipv4: bool,
    pub enable_ipv6: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: String::from("cilium-agent"),
            node_name: String::from("localhost"),
            cluster_name: String::from("default"),
            cluster_id: 1,
            enable_ipv4: true,
            enable_ipv6: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EbpfConfig {
    pub map_prefix: String,
    pub map_dir: PathBuf,
    pub map_max_entries: u32,
}

impl Default for EbpfConfig {
    fn default() -> Self {
        Self {
            map_prefix: String::from("cilium_"),
            map_dir: PathBuf::from("/sys/fs/bpf"),
            map_max_entries: 65536,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub mtu: u16,
    pub enable_ipv4: bool,
    pub enable_ipv6: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mtu: 1500,
            enable_ipv4: true,
            enable_ipv6: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = format!(
            "seriousum-core-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        );
        path.push(nonce);
        path
    }

    #[test]
    fn default_config() {
        let c = Config::default();
        assert_eq!(c.agent.name, "cilium-agent");
    }

    #[test]
    fn config_round_trips_through_save_and_load() {
        let path = unique_path("roundtrip.json");
        let config = Config {
            agent: AgentConfig {
                name: String::from("seriousum-agent"),
                node_name: String::from("node-a"),
                cluster_name: String::from("cluster-a"),
                cluster_id: 42,
                enable_ipv4: false,
                enable_ipv6: true,
            },
            ebpf: EbpfConfig {
                map_prefix: String::from("seriousum_"),
                map_dir: PathBuf::from("/tmp/bpf"),
                map_max_entries: 1024,
            },
            network: NetworkConfig {
                mtu: 9000,
                enable_ipv4: false,
                enable_ipv6: true,
            },
            options: BTreeMap::from([(String::from("feature"), String::from("enabled"))]),
        };

        config.save(&path).expect("save config");
        let loaded = Config::load(&path).expect("load config");

        assert_eq!(loaded.agent.name, config.agent.name);
        assert_eq!(loaded.agent.node_name, config.agent.node_name);
        assert_eq!(loaded.agent.cluster_name, config.agent.cluster_name);
        assert_eq!(loaded.agent.cluster_id, config.agent.cluster_id);
        assert_eq!(loaded.agent.enable_ipv4, config.agent.enable_ipv4);
        assert_eq!(loaded.agent.enable_ipv6, config.agent.enable_ipv6);
        assert_eq!(loaded.ebpf.map_prefix, config.ebpf.map_prefix);
        assert_eq!(loaded.ebpf.map_dir, config.ebpf.map_dir);
        assert_eq!(loaded.ebpf.map_max_entries, config.ebpf.map_max_entries);
        assert_eq!(loaded.network.mtu, config.network.mtu);
        assert_eq!(loaded.network.enable_ipv4, config.network.enable_ipv4);
        assert_eq!(loaded.network.enable_ipv6, config.network.enable_ipv6);
        assert_eq!(loaded.options, config.options);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_config_loads_with_defaults() {
        let path = unique_path("partial.json");
        std::fs::write(
            &path,
            r#"{
                "agent": { "name": "custom-agent" },
                "network": { "mtu": 9000 },
                "options": { "mode": "test" }
            }"#,
        )
        .expect("write partial config");

        let loaded = Config::load(&path).expect("load partial config");
        assert_eq!(loaded.agent.name, "custom-agent");
        assert_eq!(loaded.agent.node_name, "localhost");
        assert_eq!(loaded.agent.cluster_name, "default");
        assert_eq!(loaded.agent.cluster_id, 1);
        assert!(loaded.agent.enable_ipv4);
        assert!(!loaded.agent.enable_ipv6);
        assert_eq!(loaded.ebpf, EbpfConfig::default());
        assert_eq!(loaded.network.mtu, 9000);
        assert!(loaded.network.enable_ipv4);
        assert!(!loaded.network.enable_ipv6);
        assert_eq!(loaded.options.get("mode"), Some(&String::from("test")));

        let _ = std::fs::remove_file(&path);
    }
}
