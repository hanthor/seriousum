use std::fmt;

use seriousum_core::{
    BPF_MAP_MAX_ENTRIES, CONNTRACK_MAP_NAME, DEFAULT_MTU, ENDPOINT_MAP_NAME, ENDPOINT_PREFIX,
    IPCACHE_MAP_NAME, NAT_MAP_NAME, POLICY_MAP_NAME,
};

/// Convenience result type for the datapath scaffold.
pub type Result<T> = seriousum_core::Result<T>;

/// Minimal datapath configuration for the scaffold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatapathConfig {
    /// Prefix used for BPF map names.
    pub map_prefix: String,
    /// Prefix used for endpoint interfaces.
    pub endpoint_prefix: String,
    /// Default MTU used by datapath objects.
    pub mtu: u16,
    /// Maximum entries used for map scaffolding.
    pub map_max_entries: u32,
}

impl Default for DatapathConfig {
    fn default() -> Self {
        Self {
            map_prefix: String::from("cilium_"),
            endpoint_prefix: String::from(ENDPOINT_PREFIX),
            mtu: DEFAULT_MTU,
            map_max_entries: BPF_MAP_MAX_ENTRIES,
        }
    }
}

/// Minimal datapath scaffold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datapath {
    config: DatapathConfig,
}

impl Datapath {
    /// Create a new datapath scaffold from a config.
    pub fn new(config: DatapathConfig) -> Self {
        Self { config }
    }

    /// Create a default scaffold.
    pub fn scaffold() -> Self {
        Self::new(DatapathConfig::default())
    }

    /// Access the datapath configuration.
    pub fn config(&self) -> &DatapathConfig {
        &self.config
    }

    /// Return the canonical map names this scaffold expects to manage.
    pub fn map_names(&self) -> Vec<String> {
        [
            ENDPOINT_MAP_NAME,
            IPCACHE_MAP_NAME,
            POLICY_MAP_NAME,
            CONNTRACK_MAP_NAME,
            NAT_MAP_NAME,
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    }

    /// Build a compact report for the scaffold.
    pub fn report(&self) -> DatapathReport {
        DatapathReport {
            map_prefix: self.config.map_prefix.clone(),
            endpoint_prefix: self.config.endpoint_prefix.clone(),
            mtu: self.config.mtu,
            map_max_entries: self.config.map_max_entries,
            maps: self.map_names(),
        }
    }

    /// Render a one-line summary.
    pub fn summary(&self) -> String {
        self.report().to_string()
    }
}

/// Datapath report rendered by the thin binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatapathReport {
    /// Prefix used for BPF map names.
    pub map_prefix: String,
    /// Prefix used for endpoint interfaces.
    pub endpoint_prefix: String,
    /// Default MTU used by datapath objects.
    pub mtu: u16,
    /// Maximum entries used for map scaffolding.
    pub map_max_entries: u32,
    /// Canonical map names.
    pub maps: Vec<String>,
}

impl fmt::Display for DatapathReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "datapath scaffold ready | map_prefix={} | endpoint_prefix={} | mtu={} | max_entries={} | maps={}",
            self.map_prefix,
            self.endpoint_prefix,
            self.mtu,
            self.map_max_entries,
            self.maps.join(", "),
        )
    }
}

/// Run the datapath scaffold.
pub fn run() -> Result<String> {
    Ok(Datapath::scaffold().summary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scaffold_uses_shared_core_constants() {
        let datapath = Datapath::scaffold();

        assert_eq!(datapath.config().endpoint_prefix, ENDPOINT_PREFIX);
        assert_eq!(datapath.config().mtu, DEFAULT_MTU);
        assert_eq!(datapath.config().map_max_entries, BPF_MAP_MAX_ENTRIES);
    }

    #[test]
    fn report_lists_canonical_maps() {
        let datapath = Datapath::scaffold();
        let report = datapath.report();

        assert_eq!(
            report.maps,
            vec![
                ENDPOINT_MAP_NAME,
                IPCACHE_MAP_NAME,
                POLICY_MAP_NAME,
                CONNTRACK_MAP_NAME,
                NAT_MAP_NAME,
            ]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
        );
        assert!(report.to_string().contains("datapath scaffold ready"));
    }

    #[test]
    fn run_returns_summary() {
        let output = run().expect("run datapath scaffold");

        assert!(output.contains("map_prefix=cilium_"));
        assert!(output.contains(ENDPOINT_MAP_NAME));
        assert!(output.contains(NAT_MAP_NAME));
    }
}
