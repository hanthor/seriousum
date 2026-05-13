//! Configuration helpers and typed configuration model.

use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

use ipnet::IpNet;

pub use seriousum_core::config::{
    AgentConfig, Config as RuntimeConfig, EbpfConfig, NetworkConfig,
};

/// Prefix used for generated environment variable names.
pub const CILIUM_ENV_PREFIX: &str = "CILIUM_";

/// Routing mode where encapsulation is disabled.
pub const ROUTING_MODE_NATIVE: &str = "native";
/// Routing mode where encapsulation is enabled.
pub const ROUTING_MODE_TUNNEL: &str = "tunnel";

/// IPAM mode for AWS ENI.
pub const IPAM_ENI: &str = "eni";
/// IPAM mode for Alibaba Cloud.
pub const IPAM_ALIBABA_CLOUD: &str = "alibabacloud";
/// Delegated plugin IPAM mode.
pub const IPAM_DELEGATED_PLUGIN: &str = "delegated-plugin";

/// Minimum auth map entries.
pub const AUTH_MAP_ENTRIES_MIN: i32 = 1 << 8;
/// Maximum auth map entries.
pub const AUTH_MAP_ENTRIES_MAX: i32 = 1 << 24;
/// Default auth map entries.
pub const AUTH_MAP_ENTRIES_DEFAULT: i32 = 1 << 19;

/// Default global TCP CT map entries.
pub const CT_MAP_ENTRIES_GLOBAL_TCP_DEFAULT: i32 = 2 << 18;
/// Default global non-TCP CT map entries.
pub const CT_MAP_ENTRIES_GLOBAL_ANY_DEFAULT: i32 = 2 << 17;

/// Minimum CT/NAT table size.
pub const LIMIT_TABLE_MIN: i32 = 1 << 10;
/// Maximum CT/NAT table size.
pub const LIMIT_TABLE_MAX: i32 = 1 << 24;

/// Minimum fragment-tracking map size.
pub const FRAGMENTS_MAP_MIN: i32 = 1 << 8;
/// Maximum fragment-tracking map size.
pub const FRAGMENTS_MAP_MAX: i32 = 1 << 16;
/// Default fragment-tracking map size from Cilium defaults.
pub const FRAGMENTS_MAP_ENTRIES_DEFAULT: i32 = 8_192;

/// Default NAT map size (2/3 of full CT size).
pub const NAT_MAP_ENTRIES_GLOBAL_DEFAULT: i32 =
    ((CT_MAP_ENTRIES_GLOBAL_TCP_DEFAULT + CT_MAP_ENTRIES_GLOBAL_ANY_DEFAULT) * 2) / 3;


/// Where a config value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConfigSource {
    /// Built-in default value.
    Default,
    /// Loaded from a config file.
    File,
    /// Loaded from an environment variable.
    EnvVar,
    /// Loaded from a CLI flag.
    Flag,
    /// Applied as a programmatic override.
    Override,
}

/// A single configuration value with provenance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConfigValue {
    /// Raw string value.
    pub raw: String,
    /// Source of the raw value.
    pub source: ConfigSource,
}

impl ConfigValue {
    /// Creates a new configuration value.
    pub fn new(raw: impl Into<String>, source: ConfigSource) -> Self {
        Self {
            raw: raw.into(),
            source,
        }
    }

    /// Returns the raw value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Parses the raw value into a typed value.
    pub fn parse<T: std::str::FromStr>(&self) -> Result<T, ConfigError>
    where
        T::Err: std::fmt::Display,
    {
        self.raw.parse::<T>().map_err(|e| ConfigError::ParseError {
            key: String::new(),
            value: self.raw.clone(),
            msg: e.to_string(),
        })
    }
}

/// Flat key-value configuration registry with source tracking.
#[derive(Debug, Default)]
pub struct Config {
    values: HashMap<String, ConfigValue>,
}

impl Config {
    /// Creates an empty configuration registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a key to a string value with provenance.
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        source: ConfigSource,
    ) {
        self.values
            .insert(key.into(), ConfigValue::new(value, source));
    }

    /// Returns the stored value for a key.
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }

    /// Returns the stored raw string for a key.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(ConfigValue::as_str)
    }

    /// Returns the stored value parsed as a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key)?.parse::<bool>().ok()
    }

    /// Returns the stored value parsed as an unsigned integer.
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.values.get(key)?.parse::<u64>().ok()
    }

    /// Returns the stored value or the provided default.
    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.values
            .get(key)
            .map(|value| value.raw.clone())
            .unwrap_or_else(|| default.to_string())
    }

    /// Returns whether a key is present.
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Returns the number of stored keys.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Merges another config into this one.
    pub fn merge(&mut self, other: Config) {
        for (k, v) in other.values {
            self.values.insert(k, v);
        }
    }
}

/// Well-known Cilium configuration key names mirrored from `option/config.go`.
pub mod keys {
    /// Enables IPv4 support.
    pub const ENABLE_IPV4: &str = "enable-ipv4";
    /// Enables IPv6 support.
    pub const ENABLE_IPV6: &str = "enable-ipv6";
    /// Selects the tunnel mode.
    pub const TUNNEL_MODE: &str = "tunnel";
    /// Enables Hubble.
    pub const ENABLE_HUBBLE: &str = "enable-hubble";
    /// Enables policy enforcement.
    pub const ENABLE_POLICY: &str = "enable-policy";
    /// Configures the agent health port.
    pub const AGENT_HEALTH_PORT: &str = "agent-health-port";
    /// Configures the cluster name.
    pub const CLUSTER_NAME: &str = "cluster-name";
    /// Configures the cluster ID.
    pub const CLUSTER_ID: &str = "cluster-id";
    /// Selects kube-proxy replacement mode.
    pub const KUBE_PROXY_REPLACEMENT: &str = "kube-proxy-replacement";
    /// Enables the bandwidth manager.
    pub const ENABLE_BANDWIDTH_MANAGER: &str = "enable-bandwidth-manager";
    /// Enables encryption.
    pub const ENABLE_ENCRYPTION: &str = "enable-encryption";
    /// Selects the encryption type.
    pub const ENCRYPTION_TYPE: &str = "encryption";
    /// Selects the NodePort mode.
    pub const NODE_PORT_MODE: &str = "node-port-mode";
    /// Selects the IPAM mode.
    pub const IPAM_MODE: &str = "ipam";
}

/// Errors returned by config registry helpers.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required key was missing.
    #[error("required key missing: {0}")]
    Missing(String),
    /// A stored value could not be parsed.
    #[error("parse error for key {key}={value}: {msg}")]
    ParseError { key: String, value: String, msg: String },
    /// A stored value was syntactically valid but semantically rejected.
    #[error("invalid value {value} for key {key}: {reason}")]
    InvalidValue {
        key: String,
        value: String,
        reason: String,
    },
}

/// Errors for parity helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityError(pub String);

impl Display for ParityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParityError {}

/// Returns the default configuration.
pub fn default_config() -> RuntimeConfig {
    RuntimeConfig::default()
}

/// Port of Cilium's `getEnvName()`.
pub fn get_env_name(option: &str) -> String {
    format!(
        "{CILIUM_ENV_PREFIX}{}",
        option.replace('-', "_").to_ascii_uppercase()
    )
}

/// Port of Cilium's `BindEnv`: returns the environment name for an option.
pub fn bind_env(opt_name: &str) -> String {
    get_env_name(opt_name)
}

/// Port of Cilium's `BindEnvWithLegacyEnvFallback` environment selection logic.
pub fn bind_env_with_legacy_env_fallback<F>(
    opt_name: &str,
    legacy_env_name: &str,
    lookup: F,
) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let env_name = get_env_name(opt_name);
    if lookup(&env_name).is_some_and(|v| !v.is_empty()) {
        env_name
    } else {
        legacy_env_name.to_string()
    }
}

/// Port of Cilium's `ReadDirConfig`.
pub fn read_dir_config(dir_name: &Path) -> Result<BTreeMap<String, String>, ParityError> {
    let mut out = BTreeMap::new();
    let files = match std::fs::read_dir(dir_name) {
        Ok(files) => files,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => {
            return Err(ParityError(format!(
                "unable to read configuration directory: {err}"
            )));
        }
    };

    for file in files {
        let Ok(file) = file else {
            continue;
        };

        let mut path = file.path();
        let Ok(ty) = file.file_type() else {
            continue;
        };
        if ty.is_dir() {
            continue;
        }

        if !ty.is_symlink() {
            path = match std::fs::canonicalize(&path) {
                Ok(path) => path,
                Err(_) => continue,
            };
        }

        let Ok(md) = std::fs::metadata(&path) else {
            continue;
        };
        if md.is_dir() {
            continue;
        }

        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        out.insert(
            file.file_name().to_string_lossy().into_owned(),
            String::from_utf8_lossy(&bytes).trim().to_string(),
        );
    }

    Ok(out)
}

/// Port of Cilium's IPv6 cluster allocation CIDR validation.
pub fn validate_ipv6_cluster_alloc_cidr(cidr: &str) -> Result<String, ParityError> {
    let (address, prefix) = cidr.split_once('/').ok_or_else(|| {
        ParityError("invalid IPv6 cluster allocation CIDR: missing prefix length".to_string())
    })?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|e| ParityError(format!("invalid IPv6 cluster allocation CIDR: {e}")))?;
    if prefix != 64 {
        return Err(ParityError("Prefix length must be /64".to_string()));
    }

    let ip = address
        .parse::<Ipv6Addr>()
        .map_err(|e| ParityError(format!("invalid IPv6 cluster allocation CIDR: {e}")))?;
    let masked = Ipv6Addr::from(u128::from(ip) & (!0u128 << 64));
    Ok(masked.to_string())
}

/// Subset of daemon config used for option parity checks.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityDaemonConfig {
    /// Enables IPv4.
    pub enable_ipv4: bool,
    /// Enables IPv6.
    pub enable_ipv6: bool,
    /// Enables SCTP.
    pub enable_sctp: bool,
    /// IPAM mode.
    pub ipam: String,
    /// Excluded local address ranges.
    pub exclude_local_addresses: Vec<IpNet>,
    /// Auth map entries.
    pub auth_map_entries: i32,
    /// Global TCP CT map entries.
    pub ct_map_entries_global_tcp: i32,
    /// Global non-TCP CT map entries.
    pub ct_map_entries_global_any: i32,
    /// Global NAT map entries.
    pub nat_map_entries_global: i32,
    /// Global neighbor map entries.
    pub neigh_map_entries_global: i32,
    /// Fragment map entries.
    pub fragments_map_entries: i32,
    /// IPv4 native routing CIDR.
    pub ipv4_native_routing_cidr: Option<IpNet>,
    /// IPv6 native routing CIDR.
    pub ipv6_native_routing_cidr: Option<IpNet>,
    /// Enables IPv4 masquerading.
    pub enable_ipv4_masquerade: bool,
    /// Enables IPv6 masquerading.
    pub enable_ipv6_masquerade: bool,
    /// Routing mode.
    pub routing_mode: String,
    /// Enables IP masquerading agent.
    pub enable_ip_masq_agent: bool,
    /// Local router IPv4.
    pub local_router_ipv4: String,
    /// Local router IPv6.
    pub local_router_ipv6: String,
    /// Enables endpoint health checking.
    pub enable_endpoint_health_checking: bool,
    /// Enables envoy config.
    pub enable_envoy_config: bool,
}

impl Default for ParityDaemonConfig {
    fn default() -> Self {
        Self {
            enable_ipv4: false,
            enable_ipv6: false,
            enable_sctp: false,
            ipam: String::new(),
            exclude_local_addresses: Vec::new(),
            auth_map_entries: AUTH_MAP_ENTRIES_DEFAULT,
            ct_map_entries_global_tcp: CT_MAP_ENTRIES_GLOBAL_TCP_DEFAULT,
            ct_map_entries_global_any: CT_MAP_ENTRIES_GLOBAL_ANY_DEFAULT,
            nat_map_entries_global: NAT_MAP_ENTRIES_GLOBAL_DEFAULT,
            neigh_map_entries_global: NAT_MAP_ENTRIES_GLOBAL_DEFAULT,
            fragments_map_entries: FRAGMENTS_MAP_ENTRIES_DEFAULT,
            ipv4_native_routing_cidr: None,
            ipv6_native_routing_cidr: None,
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_TUNNEL.to_string(),
            enable_ip_masq_agent: false,
            local_router_ipv4: String::new(),
            local_router_ipv6: String::new(),
            enable_endpoint_health_checking: false,
            enable_envoy_config: false,
        }
    }
}

impl ParityDaemonConfig {
    /// Port of Cilium's `IPv4Enabled()`.
    pub const fn ipv4_enabled(&self) -> bool {
        self.enable_ipv4
    }

    /// Port of Cilium's `IPv6Enabled()`.
    pub const fn ipv6_enabled(&self) -> bool {
        self.enable_ipv6
    }

    /// Port of Cilium's `SCTPEnabled()`.
    pub const fn sctp_enabled(&self) -> bool {
        self.enable_sctp
    }

    /// Port of Cilium's `IPAMMode()`.
    pub fn ipam_mode(&self) -> &str {
        &self.ipam
    }

    /// Port of Cilium's excluded-local-address parser.
    pub fn parse_excluded_local_addresses<I, S>(&mut self, values: I) -> Result<(), ParityError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for ip_string in values {
            let text = ip_string.as_ref();
            let prefix = text.parse::<IpNet>().map_err(|e| {
                ParityError(format!(
                    "unable to parse excluded local address {text}: {e}"
                ))
            })?;
            self.exclude_local_addresses.push(prefix);
        }
        Ok(())
    }

    /// Port of Cilium's excluded-local-address membership check.
    pub fn is_excluded_local_address(&self, addr: IpAddr) -> bool {
        self.exclude_local_addresses
            .iter()
            .any(|prefix| prefix.contains(&addr))
    }

    /// Port of Cilium's `TunnelingEnabled()`.
    pub fn tunneling_enabled(&self) -> bool {
        self.routing_mode != ROUTING_MODE_NATIVE
    }

    /// Port of Cilium's `checkMapSizeLimits()`.
    pub fn check_map_size_limits(&mut self) -> Result<(), ParityError> {
        if self.auth_map_entries < AUTH_MAP_ENTRIES_MIN {
            return Err(ParityError(format!(
                "specified AuthMap max entries {} must be greater or equal to {}",
                self.auth_map_entries, AUTH_MAP_ENTRIES_MIN
            )));
        }
        if self.auth_map_entries > AUTH_MAP_ENTRIES_MAX {
            return Err(ParityError(format!(
                "specified AuthMap max entries {} must not exceed maximum {}",
                self.auth_map_entries, AUTH_MAP_ENTRIES_MAX
            )));
        }

        if self.ct_map_entries_global_tcp < LIMIT_TABLE_MIN
            || self.ct_map_entries_global_any < LIMIT_TABLE_MIN
        {
            return Err(ParityError(format!(
                "specified CT tables values {}/{} must be greater or equal to {}",
                self.ct_map_entries_global_tcp, self.ct_map_entries_global_any, LIMIT_TABLE_MIN
            )));
        }
        if self.ct_map_entries_global_tcp > LIMIT_TABLE_MAX
            || self.ct_map_entries_global_any > LIMIT_TABLE_MAX
        {
            return Err(ParityError(format!(
                "specified CT tables values {}/{} must not exceed maximum {}",
                self.ct_map_entries_global_tcp, self.ct_map_entries_global_any, LIMIT_TABLE_MAX
            )));
        }

        if self.nat_map_entries_global < LIMIT_TABLE_MIN {
            return Err(ParityError(format!(
                "specified NAT table size {} must be greater or equal to {}",
                self.nat_map_entries_global, LIMIT_TABLE_MIN
            )));
        }
        if self.nat_map_entries_global > LIMIT_TABLE_MAX {
            return Err(ParityError(format!(
                "specified NAT tables size {} must not exceed maximum {}",
                self.nat_map_entries_global, LIMIT_TABLE_MAX
            )));
        }
        let ct_total = self.ct_map_entries_global_tcp + self.ct_map_entries_global_any;
        if self.nat_map_entries_global > ct_total {
            if self.nat_map_entries_global == NAT_MAP_ENTRIES_GLOBAL_DEFAULT {
                self.nat_map_entries_global = (ct_total * 2) / 3;
            } else {
                return Err(ParityError(format!(
                    "specified NAT tables size {} must not exceed maximum CT table size {}",
                    self.nat_map_entries_global, ct_total
                )));
            }
        }

        if self.fragments_map_entries < FRAGMENTS_MAP_MIN {
            return Err(ParityError(format!(
                "specified max entries {} for fragment-tracking map must be greater or equal to {}",
                self.fragments_map_entries, FRAGMENTS_MAP_MIN
            )));
        }
        if self.fragments_map_entries > FRAGMENTS_MAP_MAX {
            return Err(ParityError(format!(
                "specified max entries {} for fragment-tracking map must not exceed maximum {}",
                self.fragments_map_entries, FRAGMENTS_MAP_MAX
            )));
        }

        Ok(())
    }

    /// Port of Cilium's `checkIPv4NativeRoutingCIDR()`.
    pub fn check_ipv4_native_routing_cidr(&self) -> Result<(), ParityError> {
        if self.ipv4_native_routing_cidr.is_some() {
            return Ok(());
        }
        if !self.enable_ipv4 || !self.enable_ipv4_masquerade {
            return Ok(());
        }
        if self.enable_ip_masq_agent {
            return Ok(());
        }
        if self.tunneling_enabled() {
            return Ok(());
        }
        if self.ipam_mode() == IPAM_ENI || self.ipam_mode() == IPAM_ALIBABA_CLOUD {
            return Ok(());
        }

        Err(ParityError(format!(
            "native routing cidr must be configured with option --ipv4-native-routing-cidr \
in combination with --enable-ipv4=true --enable-ipv4-masquerade=true \
--enable-ip-masq-agent=false --routing-mode=native --ipam={}",
            self.ipam_mode()
        )))
    }

    /// Port of Cilium's `checkIPv6NativeRoutingCIDR()`.
    pub fn check_ipv6_native_routing_cidr(&self) -> Result<(), ParityError> {
        if self.ipv6_native_routing_cidr.is_some() {
            return Ok(());
        }
        if !self.enable_ipv6 || !self.enable_ipv6_masquerade {
            return Ok(());
        }
        if self.enable_ip_masq_agent {
            return Ok(());
        }
        if self.tunneling_enabled() {
            return Ok(());
        }

        Err(ParityError(
            "native routing cidr must be configured with option --ipv6-native-routing-cidr \
in combination with --enable-ipv6=true --enable-ipv6-masquerade=true \
--enable-ip-masq-agent=false --routing-mode=native"
                .to_string(),
        ))
    }

    /// Port of Cilium's `checkIPAMDelegatedPlugin()`.
    pub fn check_ipam_delegated_plugin(&self) -> Result<(), ParityError> {
        if self.ipam == IPAM_DELEGATED_PLUGIN {
            if self.enable_ipv4 && self.local_router_ipv4.is_empty() {
                return Err(ParityError(
                    "--local-router-ipv4 must be provided when IPv4 is enabled with --ipam=delegated-plugin"
                        .to_string(),
                ));
            }
            if self.enable_ipv6 && self.local_router_ipv6.is_empty() {
                return Err(ParityError(
                    "--local-router-ipv6 must be provided when IPv6 is enabled with --ipam=delegated-plugin"
                        .to_string(),
                ));
            }
            if self.enable_endpoint_health_checking {
                return Err(ParityError(
                    "--enable-endpoint-health-checking must be disabled with --ipam=delegated-plugin"
                        .to_string(),
                ));
            }
            if self.enable_envoy_config {
                return Err(ParityError(
                    "--enable-envoy-config must be disabled with --ipam=delegated-plugin"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod parity_tests {
    use std::collections::BTreeMap;
    use std::net::IpAddr;
    use std::path::PathBuf;
    use std::str::FromStr;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        PathBuf::from("target")
            .join("config-parity-tests")
            .join(format!("{name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn parity_test_validate_ipv6_cluster_alloc_cidr() {
        let base = validate_ipv6_cluster_alloc_cidr("fdfd::/64").expect("valid /64");
        assert_eq!(base, "fdfd::");

        let base =
            validate_ipv6_cluster_alloc_cidr("fdfd:fdfd:fdfd:fdfd:aaaa::/64").expect("valid /64");
        assert_eq!(base, "fdfd:fdfd:fdfd:fdfd::");

        assert!(validate_ipv6_cluster_alloc_cidr("foo").is_err());
        assert!(validate_ipv6_cluster_alloc_cidr("fdfd").is_err());
        assert!(validate_ipv6_cluster_alloc_cidr("fdfd::/32").is_err());
        assert!(validate_ipv6_cluster_alloc_cidr("").is_err());
    }

    #[test]
    fn parity_test_get_env_name() {
        let cases = [
            ("foo", "CILIUM_FOO"),
            ("FOO", "CILIUM_FOO"),
            ("2222", "CILIUM_2222"),
            ("22ada22", "CILIUM_22ADA22"),
            ("22ada2------2", "CILIUM_22ADA2______2"),
            (
                "conntrack-garbage-collector-interval",
                "CILIUM_CONNTRACK_GARBAGE_COLLECTOR_INTERVAL",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(get_env_name(input), want);
        }
    }

    #[test]
    fn parity_test_read_dir_config() {
        let dir = test_dir("read-dir-config");
        std::fs::create_dir_all(&dir).expect("create test dir");

        let empty = read_dir_config(&dir).expect("read empty dir");
        assert!(empty.is_empty());

        std::fs::write(dir.join("test"), "\"1\"\n").expect("write config file");
        let one = read_dir_config(&dir).expect("read one-file dir");
        assert_eq!(
            one,
            BTreeMap::from([(String::from("test"), String::from("\"1\""))])
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parity_test_bind_env() {
        let env = BTreeMap::from([
            (String::from("LEGACY_FOO_BAR"), String::from("legacy")),
            (String::from("CILIUM_FOO_BAR"), String::from("new")),
        ]);

        let selected =
            bind_env_with_legacy_env_fallback("foo-bar", "LEGACY_FOO_BAR", |k| env.get(k).cloned());
        assert_eq!(selected, "CILIUM_FOO_BAR");

        let selected =
            bind_env_with_legacy_env_fallback("bar-foo", "LEGACY_FOO_BAR", |k| env.get(k).cloned());
        assert_eq!(selected, "LEGACY_FOO_BAR");

        assert_eq!(bind_env("foo-bar"), "CILIUM_FOO_BAR");
    }

    #[test]
    fn parity_test_enabled_functions() {
        let d = ParityDaemonConfig::default();
        assert!(!d.ipv4_enabled());
        assert!(!d.ipv6_enabled());
        assert!(!d.sctp_enabled());
        assert!(d.ipam_mode().is_empty());

        let d = ParityDaemonConfig {
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.ipv4_enabled());
        assert!(!d.ipv6_enabled());
        assert!(!d.sctp_enabled());

        let d = ParityDaemonConfig {
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert!(!d.ipv4_enabled());
        assert!(d.ipv6_enabled());
        assert!(!d.sctp_enabled());

        let d = ParityDaemonConfig {
            enable_sctp: true,
            ..ParityDaemonConfig::default()
        };
        assert!(!d.ipv4_enabled());
        assert!(!d.ipv6_enabled());
        assert!(d.sctp_enabled());

        let d = ParityDaemonConfig {
            ipam: IPAM_ENI.to_string(),
            ..ParityDaemonConfig::default()
        };
        assert_eq!(d.ipam_mode(), IPAM_ENI);
    }

    #[test]
    fn parity_test_local_address_exclusion() {
        let mut d = ParityDaemonConfig::default();
        d.parse_excluded_local_addresses(["1.1.1.1/32", "3.3.3.0/24", "f00d::1/128"])
            .expect("parse excluded addresses");

        assert!(d.is_excluded_local_address(IpAddr::from_str("1.1.1.1").expect("valid ip")));
        assert!(!d.is_excluded_local_address(IpAddr::from_str("1.1.1.2").expect("valid ip")));
        assert!(d.is_excluded_local_address(IpAddr::from_str("3.3.3.1").expect("valid ip")));
        assert!(d.is_excluded_local_address(IpAddr::from_str("f00d::1").expect("valid ip")));
        assert!(!d.is_excluded_local_address(IpAddr::from_str("f00d::2").expect("valid ip")));
    }

    #[test]
    fn parity_test_check_map_size_limits() {
        let mut d = ParityDaemonConfig::default();
        d.check_map_size_limits()
            .expect("default limits should pass");

        let mut d = ParityDaemonConfig {
            auth_map_entries: AUTH_MAP_ENTRIES_MIN - 1,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_map_size_limits().is_err());

        let mut d = ParityDaemonConfig {
            auth_map_entries: AUTH_MAP_ENTRIES_MAX + 1,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_map_size_limits().is_err());

        let mut d = ParityDaemonConfig {
            ct_map_entries_global_tcp: LIMIT_TABLE_MIN - 1,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_map_size_limits().is_err());

        let mut d = ParityDaemonConfig {
            ct_map_entries_global_any: LIMIT_TABLE_MAX + 1,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_map_size_limits().is_err());

        let mut d = ParityDaemonConfig {
            ct_map_entries_global_tcp: 2_048,
            ct_map_entries_global_any: 4_096,
            nat_map_entries_global: NAT_MAP_ENTRIES_GLOBAL_DEFAULT,
            ..ParityDaemonConfig::default()
        };
        d.check_map_size_limits()
            .expect("NAT auto-size should succeed");
        assert_eq!(d.nat_map_entries_global, ((2_048 + 4_096) * 2) / 3);

        let mut d = ParityDaemonConfig {
            ct_map_entries_global_tcp: 2_048,
            ct_map_entries_global_any: 4_096,
            nat_map_entries_global: 8_192,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_map_size_limits().is_err());
    }

    #[test]
    fn parity_test_check_ipv4_native_routing_cidr() {
        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipam: String::from("azure"),
            ipv4_native_routing_cidr: Some("10.127.64.0/18".parse().expect("valid cidr")),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: false,
            enable_ipv6_masquerade: false,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipam: String::from("azure"),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_TUNNEL.to_string(),
            ipam: String::from("azure"),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipam: IPAM_ENI.to_string(),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipam: String::from("azure"),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_err());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipam: String::from("kubernetes"),
            enable_ipv4: true,
            enable_ip_masq_agent: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv4_native_routing_cidr().is_ok());
    }

    #[test]
    fn parity_test_check_ipv6_native_routing_cidr() {
        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            ipv6_native_routing_cidr: Some("fd00::/120".parse().expect("valid cidr")),
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv6_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: false,
            enable_ipv6_masquerade: false,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv6_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_TUNNEL.to_string(),
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv6_native_routing_cidr().is_ok());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv6_native_routing_cidr().is_err());

        let d = ParityDaemonConfig {
            enable_ipv4_masquerade: true,
            enable_ipv6_masquerade: true,
            routing_mode: ROUTING_MODE_NATIVE.to_string(),
            enable_ipv6: true,
            enable_ip_masq_agent: true,
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipv6_native_routing_cidr().is_ok());
    }

    #[test]
    fn parity_test_check_ipam_delegated_plugin() {
        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_ipv4: true,
            local_router_ipv4: String::from("169.254.0.0"),
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipam_delegated_plugin().is_ok());

        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_ipv6: true,
            local_router_ipv6: String::from("fe80::1"),
            ..ParityDaemonConfig::default()
        };
        assert!(d.check_ipam_delegated_plugin().is_ok());

        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_endpoint_health_checking: true,
            ..ParityDaemonConfig::default()
        };
        assert_eq!(
            d.check_ipam_delegated_plugin()
                .expect_err("health checking must be rejected")
                .to_string(),
            "--enable-endpoint-health-checking must be disabled with --ipam=delegated-plugin"
        );

        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_ipv4: true,
            ..ParityDaemonConfig::default()
        };
        assert_eq!(
            d.check_ipam_delegated_plugin()
                .expect_err("missing local router ipv4 must be rejected")
                .to_string(),
            "--local-router-ipv4 must be provided when IPv4 is enabled with --ipam=delegated-plugin"
        );

        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_ipv6: true,
            ..ParityDaemonConfig::default()
        };
        assert_eq!(
            d.check_ipam_delegated_plugin()
                .expect_err("missing local router ipv6 must be rejected")
                .to_string(),
            "--local-router-ipv6 must be provided when IPv6 is enabled with --ipam=delegated-plugin"
        );

        let d = ParityDaemonConfig {
            ipam: IPAM_DELEGATED_PLUGIN.to_string(),
            enable_envoy_config: true,
            ..ParityDaemonConfig::default()
        };
        assert_eq!(
            d.check_ipam_delegated_plugin()
                .expect_err("envoy config must be rejected")
                .to_string(),
            "--enable-envoy-config must be disabled with --ipam=delegated-plugin"
        );
    }
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

    #[test]
    fn test_config_set_get() {
        let mut cfg = Config::new();
        cfg.set("enable-ipv4", "true", ConfigSource::Flag);
        assert_eq!(cfg.get_str("enable-ipv4"), Some("true"));
        assert_eq!(cfg.get_bool("enable-ipv4"), Some(true));
    }

    #[test]
    fn test_config_get_or_default() {
        let cfg = Config::new();
        assert_eq!(cfg.get_or("missing-key", "fallback"), "fallback");
    }

    #[test]
    fn test_config_merge() {
        let mut base = Config::new();
        base.set("a", "1", ConfigSource::Default);
        base.set("b", "2", ConfigSource::Default);

        let mut overlay = Config::new();
        overlay.set("b", "overridden", ConfigSource::Flag);
        overlay.set("c", "3", ConfigSource::Flag);

        base.merge(overlay);
        assert_eq!(base.get_str("a"), Some("1"));
        assert_eq!(base.get_str("b"), Some("overridden"));
        assert_eq!(base.get_str("c"), Some("3"));
    }

    #[test]
    fn test_config_u64() {
        let mut cfg = Config::new();
        cfg.set("port", "9090", ConfigSource::EnvVar);
        assert_eq!(cfg.get_u64("port"), Some(9090));

        cfg.set("bad", "not-a-number", ConfigSource::EnvVar);
        assert_eq!(cfg.get_u64("bad"), None);
    }

    #[test]
    fn test_well_known_keys() {
        assert_eq!(keys::ENABLE_IPV4, "enable-ipv4");
        assert_eq!(keys::TUNNEL_MODE, "tunnel");
    }
}
