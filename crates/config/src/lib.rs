//! Configuration helpers and typed configuration model.

pub use seriousum_core::config::{AgentConfig, Config, EbpfConfig, NetworkConfig};

/// Returns the default configuration.
pub fn default_config() -> Config {
    Config::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_available() {
        let cfg = default_config();
        assert!(cfg.agent.enable_ipv4);
        assert_eq!(cfg.ebpf.map_prefix, "cilium_");
    }
}
