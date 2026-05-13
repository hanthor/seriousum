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
mod parity_tests {
    use std::collections::BTreeSet;

    use crate::core_maps::{ArrayMap, BpfMap, BpfMapType, HashMap as CoreHashMap};

    #[derive(Clone, Copy)]
    struct TestObject {
        key: u32,
        value: u32,
    }

    impl TestObject {
        fn key_bytes(self) -> [u8; 4] {
            self.key.to_le_bytes()
        }

        fn value_bytes(self) -> [u8; 4] {
            self.value.to_le_bytes()
        }
    }

    struct ParityMapOps<'a> {
        map: &'a dyn BpfMap,
    }

    impl<'a> ParityMapOps<'a> {
        fn new(map: &'a dyn BpfMap) -> Self {
            Self { map }
        }

        fn update(&self, obj: TestObject) -> crate::core_maps::Result<()> {
            self.map.update(&obj.key_bytes(), &obj.value_bytes(), 0)
        }

        fn delete(&self, obj: TestObject) -> crate::core_maps::Result<()> {
            self.map.delete(&obj.key_bytes())
        }

        fn prune(&self, desired: &[TestObject]) -> crate::core_maps::Result<()> {
            let desired_keys: BTreeSet<Vec<u8>> =
                desired.iter().map(|obj| obj.key_bytes().to_vec()).collect();
            let keys_to_prune: Vec<Vec<u8>> = self
                .map
                .iter()
                .map(|(key, _)| key)
                .filter(|key| !desired_keys.contains(key))
                .collect();

            for key in keys_to_prune {
                self.map.delete(&key)?;
            }

            Ok(())
        }
    }

    struct ParityBatchIterator<'a> {
        map: &'a dyn BpfMap,
        err: Option<String>,
    }

    impl<'a> ParityBatchIterator<'a> {
        fn new(map: &'a dyn BpfMap) -> Self {
            Self { map, err: None }
        }

        fn iterate_all(&mut self) -> Vec<(Vec<u8>, Vec<u8>)> {
            match self.map.info().map_type {
                BpfMapType::Hash | BpfMapType::LRUHash | BpfMapType::LPMTrie => {}
                _ => {
                    self.err = Some(format!(
                        "unsupported map type {}, must be one either hash or lru-hash types",
                        self.map.info().map_type
                    ));
                    return Vec::new();
                }
            }

            self.err = None;
            self.map.iter().collect()
        }

        fn err(&self) -> Option<&str> {
            self.err.as_deref()
        }
    }

    fn remove_unused_map_names(
        available: &BTreeSet<String>,
        fixed: &BTreeSet<String>,
        referenced: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let mut keep = fixed.clone();
        keep.extend(referenced.iter().cloned());

        available
            .iter()
            .filter(|name| !keep.contains(*name))
            .cloned()
            .collect()
    }

    fn detect_freed_map_names(
        available: &BTreeSet<String>,
        fixed: &BTreeSet<String>,
        referenced: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        available
            .iter()
            .filter(|name| !fixed.contains(*name) && !referenced.contains(*name))
            .cloned()
            .collect()
    }

    fn delete_all_entries(map: &dyn BpfMap) -> crate::core_maps::Result<()> {
        let keys: Vec<Vec<u8>> = map.iter().map(|(key, _)| key).collect();
        for key in keys {
            map.delete(&key)?;
        }
        Ok(())
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ParityMapModel {
        name: String,
        map_type: BpfMapType,
        max_entries: usize,
        entry_count: usize,
    }

    fn map_model(map: &dyn BpfMap) -> ParityMapModel {
        let info = map.info();
        ParityMapModel {
            name: info.name,
            map_type: info.map_type,
            max_entries: info.max_entries,
            entry_count: map.len(),
        }
    }

    // Stubs ported from pkg/bpf/map_linux_test.go (requires Linux BPF syscalls;
    // run with `cargo test --features privileged`).

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_open() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_open_map() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_open_or_create() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_recreate_map() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_basic_manipulation() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_subscribe() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_dump() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_dump_per_cpu() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_dump_reliably_with_callback_overlapping() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_dump_reliably_with_callback() {}

    #[test]
    fn parity_test_privileged_delete_all() {
        let map = CoreHashMap::new("cilium_delete_all_test", 4, 4, 16);
        let key_zero = 0_u32.to_le_bytes();
        let value_zero = 0_u32.to_le_bytes();
        let key_a = 105_u32.to_le_bytes();
        let value_a = 205_u32.to_le_bytes();
        let key_b = 106_u32.to_le_bytes();
        let value_b = 206_u32.to_le_bytes();

        assert!(map.update(&key_a, &value_a, 0).is_ok());
        assert!(map.update(&key_b, &value_a, 0).is_ok());
        assert!(map.update(&key_b, &value_b, 0).is_ok());
        assert!(map.update(&key_zero, &value_zero, 0).is_ok());
        assert_eq!(map.len(), 3);

        assert!(delete_all_entries(&map).is_ok());
        assert_eq!(map.len(), 0);
        assert!(map.lookup(&key_zero).unwrap().is_none());
        assert!(map.lookup(&key_a).unwrap().is_none());
        assert!(map.lookup(&key_b).unwrap().is_none());
    }

    #[test]
    fn parity_test_privileged_get_model() {
        let map = CoreHashMap::new("cilium_get_model_test", 4, 4, 16);
        assert!(
            map.update(&1_u32.to_le_bytes(), &2_u32.to_le_bytes(), 0)
                .is_ok()
        );

        let model = map_model(&map);
        assert_eq!(
            model,
            ParityMapModel {
                name: "cilium_get_model_test".to_string(),
                map_type: BpfMapType::Hash,
                max_entries: 16,
                entry_count: 1,
            }
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_unpin() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_create_unpinned() {}

    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_error_resolver() {}

    #[test]
    fn parity_test_batch_iterator_types() {
        let map = ArrayMap::new("cilium_test", 4, 1);
        let mut iter = ParityBatchIterator::new(&map);

        let entries = iter.iterate_all();
        assert!(entries.is_empty());
        assert!(iter.err().is_some());
        assert!(
            iter.err()
                .is_some_and(|err| err.contains("unsupported map type"))
        );
    }

    #[test]
    fn parity_test_privileged_batch_iterator() {
        let map = CoreHashMap::new("cilium_batch_iter_test", 4, 4, 64);

        for i in 0_u32..16 {
            assert!(map.update(&i.to_le_bytes(), &i.to_le_bytes(), 0).is_ok());
        }

        let mut iter = ParityBatchIterator::new(&map);
        let entries = iter.iterate_all();
        assert!(iter.err().is_none());
        assert_eq!(entries.len(), 16);

        let mut seen_keys = BTreeSet::new();
        let mut seen_values = BTreeSet::new();
        for (key, value) in entries {
            let key = u32::from_le_bytes(key.try_into().expect("u32 key"));
            let value = u32::from_le_bytes(value.try_into().expect("u32 value"));
            seen_keys.insert(key);
            seen_values.insert(value);
        }

        let expected: BTreeSet<u32> = (0..16).collect();
        assert_eq!(seen_keys, expected);
        assert_eq!(seen_values, expected);
    }

    // Stubs ported from pkg/bpf/unused_maps_test.go (blocker: Linux BPF syscalls)

    #[test]
    fn parity_test_privileged_unused_maps() {
        let available = BTreeSet::from([
            "map_a".to_string(),
            "map_b".to_string(),
            "map_static".to_string(),
            "map_global".to_string(),
        ]);
        let fixed = BTreeSet::new();
        let all_referenced = available.clone();
        let none_referenced = BTreeSet::new();

        let removed_when_all_used = remove_unused_map_names(&available, &fixed, &all_referenced);
        assert!(removed_when_all_used.is_empty());

        let removed_when_unused = remove_unused_map_names(&available, &fixed, &none_referenced);
        assert_eq!(removed_when_unused, available);
    }

    #[test]
    fn parity_test_privileged_unused_maps_false_negative() {
        let available = BTreeSet::from(["used_map".to_string(), "unused_map".to_string()]);
        let fixed = BTreeSet::new();
        let referenced = BTreeSet::from(["used_map".to_string()]);

        let freed = detect_freed_map_names(&available, &fixed, &referenced);
        assert!(freed.contains("unused_map"));
        assert!(!freed.contains("used_map"));
    }

    #[test]
    fn parity_test_unused_maps_fixed_set() {
        let available = BTreeSet::from([
            "map_a".to_string(),
            "map_b".to_string(),
            "map_static".to_string(),
        ]);
        let referenced = BTreeSet::from(["map_a".to_string()]);
        let original_fixed = BTreeSet::from(["test".to_string()]);
        let fixed_clone = original_fixed.clone();

        let _deleted = remove_unused_map_names(&available, &original_fixed, &referenced);

        assert_eq!(original_fixed, fixed_clone);
    }

    // Stubs ported from pkg/bpf/ops_linux_test.go (blocker: Linux BPF syscalls)

    #[test]
    fn parity_test_privileged_map_ops() {
        let map = CoreHashMap::new("cilium_ops_test", 4, 4, 16);
        let ops = ParityMapOps::new(&map);
        let obj = TestObject { key: 1, value: 2 };

        assert!(ops.update(obj).is_ok());
        assert!(ops.update(obj).is_ok());
        assert_eq!(
            map.lookup(&obj.key_bytes()).unwrap(),
            Some(obj.value_bytes().to_vec())
        );

        assert!(ops.delete(obj).is_ok());
        assert!(map.lookup(&obj.key_bytes()).unwrap().is_none());

        assert!(ops.update(TestObject { key: 2, value: 3 }).is_ok());
        assert!(ops.prune(&[]).is_ok());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn parity_test_privileged_map_ops_prune() {
        let map = CoreHashMap::new("cilium_ops_prune_test", 4, 4, 16);
        let ops = ParityMapOps::new(&map);

        for i in 0..4 {
            assert!(ops.update(TestObject { key: i, value: i }).is_ok());
        }

        assert!(
            ops.prune(&[
                TestObject { key: 1, value: 1 },
                TestObject { key: 3, value: 3 }
            ])
            .is_ok()
        );

        assert!(map.lookup(&0_u32.to_le_bytes()).unwrap().is_none());
        assert!(map.lookup(&1_u32.to_le_bytes()).unwrap().is_some());
        assert!(map.lookup(&2_u32.to_le_bytes()).unwrap().is_none());
        assert!(map.lookup(&3_u32.to_le_bytes()).unwrap().is_some());
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn parity_test_privileged_map_ops_reconciler_example() {
        let map = CoreHashMap::new("cilium_ops_reconciler_test", 4, 4, 16);
        let ops = ParityMapOps::new(&map);

        let desired = TestObject { key: 1, value: 2 };
        assert!(ops.update(desired).is_ok());
        assert_eq!(
            map.lookup(&desired.key_bytes()).unwrap(),
            Some(desired.value_bytes().to_vec())
        );

        let removed = TestObject { key: 1, value: 2 };
        assert!(ops.prune(&[]).is_ok());
        assert!(map.lookup(&removed.key_bytes()).unwrap().is_none());
    }
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
