use std::fmt;
use std::path::{Path, PathBuf};

pub use seriousum_core::ebpf::{
    AttachType, MapDescriptor, MapType, ProgDescriptor, ProgType, map_flags, prog_flags,
};
use seriousum_core::{
    BPF_MAP_MAX_ENTRIES, CONNTRACK_MAP_NAME, ENDPOINT_MAP_NAME, IPCACHE_MAP_NAME, NAT_MAP_NAME,
    POLICY_MAP_NAME,
};

// Export core map types
pub mod core_maps;
pub use core_maps::{BpfMap, BpfMapError, BpfMapType, HashMap};

// Export service maps module
pub mod maps;

/// Convenience result type for the eBPF scaffold.
pub type Result<T> = seriousum_core::Result<T>;

/// Minimal eBPF scaffold for maps and programs.
#[derive(Debug, Clone)]
pub struct EbpfScaffold {
    pin_root: PathBuf,
    maps: Vec<MapDescriptor>,
    programs: Vec<ProgDescriptor>,
}

impl Default for EbpfScaffold {
    fn default() -> Self {
        Self::new(PathBuf::from("/sys/fs/bpf"))
    }
}

impl EbpfScaffold {
    /// Create a scaffold rooted at the given BPF pin path.
    pub fn new(pin_root: impl Into<PathBuf>) -> Self {
        let pin_root = pin_root.into();
        let maps = Self::default_maps(&pin_root);
        let programs = Self::default_programs();

        Self {
            pin_root,
            maps,
            programs,
        }
    }

    /// Create the default scaffold.
    pub fn scaffold() -> Self {
        Self::default()
    }

    /// Access the BPF pin root.
    pub fn pin_root(&self) -> &Path {
        &self.pin_root
    }

    /// Access the map descriptors.
    pub fn maps(&self) -> &[MapDescriptor] {
        &self.maps
    }

    /// Access the program descriptors.
    pub fn programs(&self) -> &[ProgDescriptor] {
        &self.programs
    }

    /// Build a concise report.
    pub fn report(&self) -> EbpfReport {
        EbpfReport {
            pin_root: self.pin_root.display().to_string(),
            maps: self.maps.iter().map(|map| map.name.clone()).collect(),
            programs: self
                .programs
                .iter()
                .map(|program| program.name.clone())
                .collect(),
        }
    }

    /// Render the report as a string.
    pub fn summary(&self) -> String {
        self.report().to_string()
    }

    fn default_maps(pin_root: &Path) -> Vec<MapDescriptor> {
        [
            (ENDPOINT_MAP_NAME, MapType::Hash),
            (IPCACHE_MAP_NAME, MapType::LruHash),
            (POLICY_MAP_NAME, MapType::Hash),
            (CONNTRACK_MAP_NAME, MapType::LruHash),
            (NAT_MAP_NAME, MapType::Hash),
        ]
        .into_iter()
        .map(|(name, map_type)| {
            MapDescriptor::new(name, map_type, 4, 8, BPF_MAP_MAX_ENTRIES)
                .with_pin(pin_root.join(name).display().to_string())
        })
        .collect()
    }

    fn default_programs() -> Vec<ProgDescriptor> {
        [
            ProgDescriptor::new("xdp/ingress", ProgType::Xdp, Some(AttachType::Xdp)),
            ProgDescriptor::new("tc/egress", ProgType::SchedAct, Some(AttachType::Egress)),
        ]
        .into_iter()
        .collect()
    }
}

/// eBPF report rendered by the thin binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfReport {
    /// Pin root used for the scaffold.
    pub pin_root: String,
    /// Canonical map names.
    pub maps: Vec<String>,
    /// Canonical program names.
    pub programs: Vec<String>,
}

impl fmt::Display for EbpfReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ebpf scaffold ready | pin_root={} | maps={} | programs={}",
            self.pin_root,
            self.maps.join(", "),
            self.programs.join(", "),
        )
    }
}

/// Run the eBPF scaffold.
pub fn run() -> Result<String> {
    Ok(EbpfScaffold::scaffold().summary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_uses_canonical_maps_and_programs() {
        let scaffold = EbpfScaffold::scaffold();

        assert_eq!(scaffold.maps().len(), 5);
        assert_eq!(scaffold.programs().len(), 2);
        assert_eq!(scaffold.pin_root(), Path::new("/sys/fs/bpf"));
    }

    #[test]
    fn report_mentions_maps_and_programs() {
        let report = EbpfScaffold::scaffold().report();

        assert!(report.maps.iter().any(|map| map == ENDPOINT_MAP_NAME));
        assert!(report.maps.iter().any(|map| map == IPCACHE_MAP_NAME));
        assert!(
            report
                .programs
                .iter()
                .any(|program| program == "xdp/ingress")
        );
        assert!(report.to_string().contains("ebpf scaffold ready"));
    }

    #[test]
    fn run_returns_summary() {
        let output = run().expect("run ebpf scaffold");

        assert!(output.contains("pin_root=/sys/fs/bpf"));
        assert!(output.contains(CONNTRACK_MAP_NAME));
    }
}
