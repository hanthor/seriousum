//! Lightweight CNI scaffolds for parity-friendly model work.

pub mod plugin;

pub use plugin::{CniCommand, CniContext, CniVersionResult, PluginError, run as run_plugin};

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Identity, IpNetwork, Result, SecurityIdentity, SecurityLabel};

/// Default component name for CNI scaffolds.
pub const COMPONENT: &str = "seriousum-cni";

// ============================================================================
// NetConf — parity-portable CNI configuration parsing
// (mirrors plugins/cilium-cni/types/types.go NetConf)
// ============================================================================

/// Minimal CNI standard NetConf fields shared by both plain conf and conflist formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CniStdConf {
    /// CNI spec version.
    #[serde(rename = "cniVersion", default)]
    pub cni_version: String,
    /// Network name.
    #[serde(default)]
    pub name: String,
    /// Plugin type.
    #[serde(rename = "type", default)]
    pub plugin_type: String,
}

/// AWS ENI-specific configuration (mirrors pkg/aws/eni/types ENISpec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EniSpec {
    /// EC2 instance type.
    #[serde(
        rename = "instance-type",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub instance_type: String,
    /// Index of the first ENI interface to use.
    #[serde(
        rename = "first-interface-index",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub first_interface_index: Option<i32>,
    /// Security group IDs.
    #[serde(
        rename = "security-groups",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub security_groups: Vec<String>,
    /// Subnet IDs.
    #[serde(rename = "subnet-ids", default, skip_serializing_if = "Vec::is_empty")]
    pub subnet_ids: Vec<String>,
    /// Subnet tags.
    #[serde(
        rename = "subnet-tags",
        default,
        skip_serializing_if = "std::collections::HashMap::is_empty"
    )]
    pub subnet_tags: std::collections::HashMap<String, String>,
    /// Tags that exclude an interface.
    #[serde(
        rename = "exclude-interface-tags",
        default,
        skip_serializing_if = "std::collections::HashMap::is_empty"
    )]
    pub exclude_interface_tags: std::collections::HashMap<String, String>,
    /// VPC ID.
    #[serde(rename = "vpc-id", default, skip_serializing_if = "String::is_empty")]
    pub vpc_id: String,
    /// Availability zone.
    #[serde(
        rename = "availability-zone",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub availability_zone: String,
}

/// Azure-specific configuration (mirrors pkg/azure/types AzureSpec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AzureSpec {
    /// Network interface name.
    #[serde(
        rename = "interface-name",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub interface_name: String,
}

/// IPAM delegated plugin type (mirrors cni/pkg/types IPAM).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CniIpamType {
    /// Delegated IPAM plugin name.
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub plugin_type: String,
}

/// IPAM spec (mirrors pkg/ipam/types IPAMSpec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IpamSpec {
    /// Number of IPs to pre-allocate.
    #[serde(rename = "pre-allocate", default)]
    pub pre_allocate: i32,
}

/// Combined IPAM block in a NetConf (mirrors plugins/cilium-cni/types IPAM).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NetConfIpam {
    /// Embedded standard CNI IPAM type.
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub ipam_type: String,
    /// Number of IPs to pre-allocate.
    #[serde(rename = "pre-allocate", default)]
    pub pre_allocate: i32,
}

/// The parsed CNI network configuration (mirrors plugins/cilium-cni/types NetConf).
///
/// Supports both plain `.conf` format and conflist `.conflist` format (with `plugins` array).
/// When the conflist format is detected the first plugin with `"type": "cilium-cni"` is
/// extracted and the top-level `cniVersion` / `name` are merged in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NetConf {
    /// CNI spec version (from the top-level of a conflist, or from the plain conf).
    #[serde(
        rename = "cniVersion",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub cni_version: String,
    /// Network name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Plugin type.
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub plugin_type: String,
    /// MTU for pod interfaces.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub mtu: u32,
    /// AWS ENI configuration.
    #[serde(default, skip_serializing_if = "eni_is_empty")]
    pub eni: EniSpec,
    /// Azure configuration.
    #[serde(default, skip_serializing_if = "azure_is_empty")]
    pub azure: AzureSpec,
    /// IPAM configuration.
    #[serde(default, skip_serializing_if = "ipam_is_empty")]
    pub ipam: NetConfIpam,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn eni_is_empty(e: &EniSpec) -> bool {
    e == &EniSpec::default()
}

fn azure_is_empty(a: &AzureSpec) -> bool {
    a == &AzureSpec::default()
}

fn ipam_is_empty(i: &NetConfIpam) -> bool {
    i == &NetConfIpam::default()
}

/// Internal conflist wrapper used only during parsing.
#[derive(Debug, Deserialize)]
struct ConfList {
    #[serde(rename = "cniVersion", default)]
    cni_version: String,
    #[serde(default)]
    name: String,
}

/// Reads and parses a CNI network configuration file.
///
/// Supports both plain conf (`.conf`) and conflist (`.conflist`) formats.
/// In conflist format the first plugin of `"type": "cilium-cni"` is selected
/// and the parent `cniVersion` / `name` are merged into the result.
pub fn read_net_conf(
    path: &std::path::Path,
) -> std::result::Result<NetConf, Box<dyn std::error::Error + Send + Sync>> {
    let data = std::fs::read(path)?;
    let raw: serde_json::Value = serde_json::from_slice(&data)?;

    // Detect conflist: has a "plugins" array at the top level.
    if let Some(plugins) = raw.get("plugins").and_then(|p| p.as_array()) {
        // Parse as conflist to get top-level cniVersion + name.
        let list: ConfList = serde_json::from_slice(&data)?;

        // Find the first cilium-cni plugin.
        let plugin_val = plugins
            .iter()
            .find(|p| p.get("type").and_then(|t| t.as_str()) == Some("cilium-cni"))
            .cloned()
            .unwrap_or_else(|| plugins.first().cloned().unwrap_or(serde_json::Value::Null));

        let mut conf: NetConf = serde_json::from_value(plugin_val)?;
        // Merge top-level fields (conflist semantics).
        if !list.cni_version.is_empty() {
            conf.cni_version = list.cni_version;
        }
        if !list.name.is_empty() && conf.name.is_empty() {
            conf.name = list.name;
        }
        return Ok(conf);
    }

    // Plain conf format.
    let conf: NetConf = serde_json::from_value(raw)?;
    Ok(conf)
}

/// Pure CNI plugin data model types that do not perform namespace or netlink I/O.
pub mod pure {
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::str::FromStr;

    use ipnet::IpNet;
    use serde::{Deserialize, Serialize};

    /// CNI command type.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CNICommand {
        /// Handles `ADD` requests.
        Add,
        /// Handles `DEL` requests.
        Del,
        /// Handles `CHECK` requests.
        Check,
        /// Handles `VERSION` requests.
        Version,
    }

    impl FromStr for CNICommand {
        type Err = CNIError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "ADD" => Ok(Self::Add),
                "DEL" => Ok(Self::Del),
                "CHECK" => Ok(Self::Check),
                "VERSION" => Ok(Self::Version),
                _ => Err(CNIError::UnknownCommand(s.to_string())),
            }
        }
    }

    /// CNI network configuration parsed from plugin JSON or conflist entries.
    #[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
    pub struct NetConf {
        /// CNI spec version.
        #[serde(rename = "cniVersion", default)]
        pub cni_version: String,
        /// Network name.
        #[serde(default)]
        pub name: String,
        /// Plugin type.
        #[serde(rename = "type", default)]
        pub plugin_type: String,
        /// Delegated IPAM configuration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ipam: Option<IPAMConfig>,
        /// DNS configuration returned by the plugin.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub dns: Option<DNSConfig>,
        /// Enables verbose debugging in the plugin.
        #[serde(rename = "enable-debug", default)]
        pub enable_debug: bool,
    }

    /// Delegated IPAM configuration.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct IPAMConfig {
        /// IPAM plugin type.
        #[serde(rename = "type", default)]
        pub ipam_type: String,
    }

    /// DNS configuration returned in a CNI result.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct DNSConfig {
        /// DNS server IPs.
        #[serde(default)]
        pub nameservers: Vec<String>,
        /// Default search domain.
        #[serde(default)]
        pub domain: String,
        /// Additional search domains.
        #[serde(default)]
        pub search: Vec<String>,
        /// Resolver options.
        #[serde(default)]
        pub options: Vec<String>,
    }

    /// Arguments passed to the CNI plugin via environment variables.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CNIArgs {
        /// Requested CNI command.
        pub command: CNICommand,
        /// Container identifier.
        pub container_id: String,
        /// Path to the target network namespace.
        pub netns: String,
        /// Interface name inside the container.
        pub ifname: String,
        /// Extra `K=V;K=V` arguments.
        pub args: String,
        /// CNI plugin search path entries.
        pub path: Vec<String>,
    }

    impl CNIArgs {
        /// Parses the extra args string into a key-value map.
        #[must_use]
        pub fn parse_extra_args(&self) -> HashMap<String, String> {
            self.args
                .split(';')
                .filter_map(|segment| {
                    if segment.is_empty() {
                        return None;
                    }

                    let mut parts = segment.splitn(2, '=');
                    let key = parts.next()?;
                    let Some(value) = parts.next() else {
                        tracing::debug!(segment = %segment, "ignoring malformed CNI extra args entry");
                        return None;
                    };

                    if key.is_empty() {
                        tracing::debug!(segment = %segment, "ignoring malformed CNI extra args entry");
                        return None;
                    }

                    Some((key.to_string(), value.to_string()))
                })
                .collect()
        }
    }

    /// A configured network interface in the CNI result.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct CNIInterface {
        /// Interface name.
        pub name: String,
        /// Interface MAC address.
        pub mac: String,
        /// Sandbox network namespace path, or an empty string for host interfaces.
        pub sandbox: String,
    }

    /// An IP configuration in the CNI result.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct IPConfig {
        /// Configured address and prefix.
        pub address: IpNet,
        /// Default gateway for the address family.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub gateway: Option<IpAddr>,
        /// Index into the `interfaces` array.
        #[serde(rename = "interface", default, skip_serializing_if = "Option::is_none")]
        pub interface_index: Option<u32>,
    }

    /// Full CNI result returned by the plugin.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct CNIResult {
        /// CNI spec version.
        #[serde(rename = "cniVersion", default)]
        pub cni_version: String,
        /// Interfaces configured by the plugin.
        #[serde(default)]
        pub interfaces: Vec<CNIInterface>,
        /// Assigned IP configurations.
        #[serde(default)]
        pub ips: Vec<IPConfig>,
        /// Optional DNS settings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub dns: Option<DNSConfig>,
    }

    impl CNIResult {
        /// Creates an empty CNI result for the provided spec version.
        #[must_use]
        pub fn new(version: impl Into<String>) -> Self {
            Self {
                cni_version: version.into(),
                interfaces: vec![],
                ips: vec![],
                dns: None,
            }
        }
    }

    /// Metadata about a chained plugin invocation.
    #[derive(Debug, Clone)]
    pub struct ChainingInfo {
        /// Previous plugin result, if one was provided.
        pub prev_result: Option<CNIResult>,
        /// Container identifier.
        pub container_id: String,
        /// Path to the target network namespace.
        pub netns: String,
        /// Interface name inside the container.
        pub ifname: String,
    }

    /// Trait implemented by pure CNI chaining plugins.
    pub trait ChainingPlugin: Send + Sync {
        /// Returns the chaining plugin name.
        fn name(&self) -> &str;

        /// Handles a CNI `ADD` request and may augment the previous result.
        fn add(&self, args: &CNIArgs, info: &ChainingInfo) -> Result<CNIResult, CNIError>;

        /// Handles a CNI `DEL` request.
        fn delete(&self, args: &CNIArgs) -> Result<(), CNIError>;
    }

    /// Errors returned by pure CNI data model helpers.
    #[derive(Debug, thiserror::Error)]
    pub enum CNIError {
        /// The provided command is not part of the CNI spec.
        #[error("unknown CNI command: {0}")]
        UnknownCommand(String),
        /// The plugin configuration is invalid.
        #[error("invalid configuration: {0}")]
        InvalidConfig(String),
        /// Network namespace handling failed.
        #[error("network namespace error: {0}")]
        Netns(String),
        /// Delegated IPAM handling failed.
        #[error("IPAM error: {0}")]
        IPAM(String),
        /// A generic plugin error with a CNI error code.
        #[error("plugin error: code={code}, msg={msg}")]
        Plugin { code: u32, msg: String },
    }
}

/// CNI operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CniOperation {
    /// Add a pod to the network.
    Add,
    /// Check the pod network.
    Check,
    /// Delete the pod network.
    Delete,
}

/// CNI lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CniState {
    /// Setup is pending.
    Pending,
    /// CNI is ready.
    Ready,
    /// CNI has been torn down.
    Deleted,
}

/// Compact CNI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniConfig {
    /// Plugin name.
    pub plugin_name: String,

    /// Pod CIDR assigned to the plugin.
    pub pod_cidr: IpNetwork,

    /// MTU for the pod interface.
    pub mtu: u32,

    /// Whether masquerading is enabled.
    pub masquerade: bool,
}

impl CniConfig {
    /// Creates a new CNI configuration.
    #[must_use]
    pub fn new(plugin_name: impl Into<String>, pod_cidr: IpNetwork) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            pod_cidr,
            mtu: 1_500,
            masquerade: true,
        }
    }

    /// Returns the default scaffold configuration.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "seriousum-cni",
            "10.42.0.0/24".parse().expect("valid cni pod cidr"),
        )
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.plugin_name.trim().is_empty() {
            return Err(Error::Cni(String::from(
                "cni plugin name must not be empty",
            )));
        }

        if self.mtu < 576 {
            return Err(Error::Cni(String::from("cni mtu must be at least 576")));
        }

        Ok(())
    }
}

impl Default for CniConfig {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// CNI session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniSession {
    /// Container identifier.
    pub container_id: String,

    /// Network namespace path.
    pub netns: String,

    /// Requested operation.
    pub operation: CniOperation,

    /// Whether the session is active.
    pub active: bool,
}

impl CniSession {
    /// Creates a new CNI session.
    #[must_use]
    pub fn new(
        container_id: impl Into<String>,
        netns: impl Into<String>,
        operation: CniOperation,
    ) -> Self {
        Self {
            container_id: container_id.into(),
            netns: netns.into(),
            operation,
            active: true,
        }
    }

    /// Returns the default scaffold session.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("container-scaffold", "/proc/self/ns/net", CniOperation::Add)
    }

    /// Marks the session inactive.
    #[must_use]
    pub fn deactivate(mut self) -> Self {
        self.active = false;
        self
    }

    /// Validates the session.
    pub fn validate(&self) -> Result<()> {
        if self.container_id.trim().is_empty() {
            return Err(Error::Cni(String::from(
                "cni container id must not be empty",
            )));
        }

        if self.netns.trim().is_empty() {
            return Err(Error::Cni(String::from("cni netns must not be empty")));
        }

        Ok(())
    }
}

impl Default for CniSession {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Compact CNI model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniModel {
    /// Identity associated with the workload.
    pub identity: Identity,

    /// CNI configuration.
    pub config: CniConfig,

    /// Session details.
    pub session: CniSession,

    /// Lifecycle state.
    pub state: CniState,
}

impl CniModel {
    /// Creates a new CNI model.
    #[must_use]
    pub fn new(identity: Identity, config: CniConfig, session: CniSession) -> Self {
        Self {
            identity,
            config,
            session,
            state: CniState::Pending,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            Identity::new(
                SecurityIdentity::unmanaged(),
                [SecurityLabel::new("cni", "scaffold")],
            ),
            CniConfig::scaffold(),
            CniSession::scaffold(),
        )
        .ready()
    }

    /// Marks the model ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.state = CniState::Ready;
        self
    }

    /// Marks the model deleted.
    #[must_use]
    pub fn deleted(mut self) -> Self {
        self.state = CniState::Deleted;
        self
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "plugin={} cidr={} active={}",
            self.config.plugin_name, self.config.pod_cidr, self.session.active
        )
    }

    /// Validates the model.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        self.session.validate()?;

        Ok(())
    }
}

impl Default for CniModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable CNI report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniReport {
    /// Component name.
    pub component: String,

    /// CNI model.
    pub cni: CniModel,

    /// Whether the CNI is ready.
    pub ready: bool,
}

impl CniReport {
    /// Builds a report from a CNI model.
    #[must_use]
    pub fn new(cni: CniModel) -> Self {
        let ready = matches!(cni.state, CniState::Ready) && cni.session.active;
        Self {
            component: COMPONENT.to_owned(),
            ready,
            cni,
        }
    }
}

/// Returns the standard CNI scaffold report.
#[must_use]
pub fn scaffold() -> CniReport {
    CniReport::new(CniModel::scaffold())
}

#[cfg(test)]
const CONNECTION_TIMEOUT: Duration = Duration::from_millis(1500);
#[cfg(test)]
const MAX_DELETION_FILES: usize = 256;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EndpointBatchDeleteRequest {
    container_id: String,
}

#[cfg(test)]
impl EndpointBatchDeleteRequest {
    fn marshal_binary(&self) -> std::result::Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum DeletionError {
    Client(String),
    ServiceUnavailable,
    Other(String),
    Queue(String),
}

#[cfg(test)]
type DeletionResult<T> = std::result::Result<T, DeletionError>;

#[cfg(test)]
trait EndpointDeletionClient: Send {
    fn endpoint_delete_many(&mut self, request: &EndpointBatchDeleteRequest) -> DeletionResult<()>;
}

#[cfg(test)]
type NewCiliumClientFn =
    Box<dyn Fn(Duration) -> DeletionResult<Box<dyn EndpointDeletionClient>> + Send + Sync>;

#[cfg(test)]
struct DeletionFallbackClient {
    cli: Option<Box<dyn EndpointDeletionClient>>,
    delete_queue_dir: PathBuf,
    delete_queue_lockfile: PathBuf,
    new_cilium_client_fn: NewCiliumClientFn,
}

#[cfg(test)]
enum DeleteBatchOutcome {
    Deleted,
    RetryQueue,
    Failed(DeletionError),
}

#[cfg(test)]
impl DeletionFallbackClient {
    fn new_for_test(
        delete_queue_dir: impl Into<PathBuf>,
        new_cilium_client_fn: NewCiliumClientFn,
    ) -> Self {
        let delete_queue_dir = delete_queue_dir.into();
        let delete_queue_lockfile = delete_queue_dir.join("lockfile");
        Self {
            cli: None,
            delete_queue_dir,
            delete_queue_lockfile,
            new_cilium_client_fn,
        }
    }

    fn try_connect(&mut self) -> DeletionResult<()> {
        if self.cli.is_none() {
            self.cli = Some((self.new_cilium_client_fn)(CONNECTION_TIMEOUT)?);
        }
        Ok(())
    }

    fn try_queue_lock(&self) -> std::io::Result<LockFile> {
        std::fs::create_dir_all(&self.delete_queue_dir)?;
        let lock = LockFile::new(&self.delete_queue_lockfile)?;
        lock.lock_shared()?;
        Ok(lock)
    }

    fn endpoint_delete_many(&mut self, request: &EndpointBatchDeleteRequest) -> DeletionResult<()> {
        match self.delete_endpoints_batch(request) {
            DeleteBatchOutcome::Deleted => Ok(()),
            DeleteBatchOutcome::Failed(error) => Err(error),
            DeleteBatchOutcome::RetryQueue => {
                let lock = self
                    .try_queue_lock()
                    .map_err(|error| DeletionError::Queue(error.to_string()))?;
                let result = match self.delete_endpoints_batch(request) {
                    DeleteBatchOutcome::Deleted => Ok(()),
                    DeleteBatchOutcome::Failed(error) => Err(error),
                    DeleteBatchOutcome::RetryQueue => {
                        self.enqueue_deletion_request_locked(request)?;
                        Ok(())
                    }
                };
                lock.unlock();
                result
            }
        }
    }

    fn delete_endpoints_batch(
        &mut self,
        request: &EndpointBatchDeleteRequest,
    ) -> DeleteBatchOutcome {
        if self.try_connect().is_err() {
            return DeleteBatchOutcome::RetryQueue;
        }

        let Some(client) = self.cli.as_mut() else {
            return DeleteBatchOutcome::RetryQueue;
        };

        match client.endpoint_delete_many(request) {
            Ok(()) => DeleteBatchOutcome::Deleted,
            Err(DeletionError::ServiceUnavailable) => DeleteBatchOutcome::RetryQueue,
            Err(error) => DeleteBatchOutcome::Failed(error),
        }
    }

    fn enqueue_deletion_request_locked(
        &self,
        request: &EndpointBatchDeleteRequest,
    ) -> DeletionResult<()> {
        let contents = request
            .marshal_binary()
            .map_err(|error| DeletionError::Other(error.to_string()))?;
        let files = std::fs::read_dir(&self.delete_queue_dir)
            .map_err(|error| DeletionError::Queue(error.to_string()))?
            .count();
        if files > MAX_DELETION_FILES {
            return Err(DeletionError::Queue(format!(
                "deletion queue directory {} has too many entries",
                self.delete_queue_dir.display()
            )));
        }

        let path = self.delete_queue_dir.join(
            deletion_request_filename(request)
                .map_err(|error| DeletionError::Queue(format!("{error:?}")))?,
        );
        std::fs::write(path, contents).map_err(|error| DeletionError::Queue(error.to_string()))
    }
}

#[cfg(test)]
fn deletion_request_filename(request: &EndpointBatchDeleteRequest) -> DeletionResult<String> {
    use std::fmt::Write as _;

    let contents = request
        .marshal_binary()
        .map_err(|error| DeletionError::Other(error.to_string()))?;
    let hash = digest(&SHA256, &contents);
    let mut filename = String::with_capacity(hash.as_ref().len() * 2 + 7);
    for byte in hash.as_ref() {
        let _ = write!(filename, "{byte:02x}");
    }
    filename.push_str(".delete");
    Ok(filename)
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockMode {
    Shared,
    Exclusive,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct LockState {
    shared_holders: usize,
    exclusive_holder: bool,
}

#[cfg(test)]
struct LockFile {
    path: PathBuf,
    mode: Mutex<Option<LockMode>>,
}

#[cfg(test)]
impl LockFile {
    fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        Ok(Self {
            path,
            mode: Mutex::new(None),
        })
    }

    fn lock_shared(&self) -> std::io::Result<()> {
        let mut registry = lock(lock_registry());
        let state = registry.entry(self.path.clone()).or_default();
        if state.exclusive_holder {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "lockfile is exclusively locked",
            ));
        }
        state.shared_holders += 1;
        *lock(&self.mode) = Some(LockMode::Shared);
        Ok(())
    }

    fn lock_exclusive(&self) -> std::io::Result<()> {
        let mut registry = lock(lock_registry());
        let state = registry.entry(self.path.clone()).or_default();
        if state.exclusive_holder || state.shared_holders > 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "lockfile is already held",
            ));
        }
        state.exclusive_holder = true;
        *lock(&self.mode) = Some(LockMode::Exclusive);
        Ok(())
    }

    fn unlock(&self) {
        let Some(mode) = lock(&self.mode).take() else {
            return;
        };

        let mut registry = lock(lock_registry());
        let mut remove_entry = false;
        if let Some(state) = registry.get_mut(&self.path) {
            match mode {
                LockMode::Shared => {
                    if state.shared_holders > 0 {
                        state.shared_holders -= 1;
                    }
                }
                LockMode::Exclusive => state.exclusive_holder = false,
            }
            remove_entry = state.shared_holders == 0 && !state.exclusive_holder;
        }
        if remove_entry {
            registry.remove(&self.path);
        }
    }
}

#[cfg(test)]
impl Drop for LockFile {
    fn drop(&mut self) {
        self.unlock();
    }
}

#[cfg(test)]
fn lock_registry() -> &'static Mutex<HashMap<PathBuf, LockState>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, LockState>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod parity_tests {
    //! Parity tests ported from plugins/cilium-cni/types/types_test.go

    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // ---- helper: write a temp file and parse it ----
    fn parse_conf(
        content: &str,
    ) -> std::result::Result<NetConf, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "seriousum-cni-parity-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::write(&path, content.as_bytes()).expect("write temp conf");
        let result = read_net_conf(&path);
        let _ = std::fs::remove_file(&path);
        result
    }

    // ---- TestReadCNIConf ----

    /// Plain conf with name + type only.
    /// Mirrors the first sub-case in TestReadCNIConf.
    #[test]
    fn parity_read_cni_conf_minimal() {
        let json = r#"{"name":"cilium","type":"cilium-cni"}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.name, "cilium");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.mtu, 0);
        assert_eq!(conf.cni_version, "");
    }

    /// Plain conf with an MTU field (9000).
    /// Mirrors the second sub-case in TestReadCNIConf.
    #[test]
    fn parity_read_cni_conf_with_mtu() {
        let json = r#"{"name":"cilium","type":"cilium-cni","mtu":9000}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.name, "cilium");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.mtu, 9000);
    }

    // ---- TestReadCNIConfENIWithPlugins ----

    /// Conflist with ENI config inside the cilium-cni plugin.
    /// Mirrors TestReadCNIConfENIWithPlugins.
    #[test]
    fn parity_read_cni_conf_eni_with_plugins() {
        let json = r#"
{
  "cniVersion":"0.3.1",
  "name":"cilium",
  "plugins": [
    {
      "cniVersion":"0.3.1",
      "type":"cilium-cni",
      "eni": {
        "first-interface-index":1,
        "security-groups":["sg-xxx"],
        "subnet-ids":["subnet-xxx"],
        "subnet-tags":{"foo":"true"},
        "exclude-interface-tags":{"baz":"false"}
      }
    }
  ]
}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.cni_version, "0.3.1");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.eni.first_interface_index, Some(1));
        assert_eq!(conf.eni.security_groups, vec!["sg-xxx"]);
        assert_eq!(conf.eni.subnet_ids, vec!["subnet-xxx"]);
        assert_eq!(
            conf.eni.subnet_tags.get("foo").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            conf.eni
                .exclude_interface_tags
                .get("baz")
                .map(String::as_str),
            Some("false")
        );
    }

    // ---- TestReadCNIConfENI ----

    /// Plain conf with full ENI config.
    /// Mirrors TestReadCNIConfENI.
    #[test]
    fn parity_read_cni_conf_eni_plain() {
        let json = r#"
{
  "name":"cilium",
  "type":"cilium-cni",
  "eni": {
    "instance-type":"m4.xlarge",
    "first-interface-index":2,
    "security-groups":["sg1","sg2"],
    "subnet-ids":["subnet-1","subnet-2"],
    "subnet-tags":{"key1":"val1","key2":"val2"},
    "exclude-interface-tags":{"key3":"val3","key4":"val4"},
    "vpc-id":"vpc-1",
    "availability-zone":"us-west1"
  }
}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.name, "cilium");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.eni.instance_type, "m4.xlarge");
        assert_eq!(conf.eni.first_interface_index, Some(2));
        assert_eq!(conf.eni.security_groups, vec!["sg1", "sg2"]);
        assert_eq!(conf.eni.subnet_ids, vec!["subnet-1", "subnet-2"]);
        assert_eq!(
            conf.eni.subnet_tags.get("key1").map(String::as_str),
            Some("val1")
        );
        assert_eq!(
            conf.eni.subnet_tags.get("key2").map(String::as_str),
            Some("val2")
        );
        assert_eq!(
            conf.eni
                .exclude_interface_tags
                .get("key3")
                .map(String::as_str),
            Some("val3")
        );
        assert_eq!(
            conf.eni
                .exclude_interface_tags
                .get("key4")
                .map(String::as_str),
            Some("val4")
        );
        assert_eq!(conf.eni.vpc_id, "vpc-1");
        assert_eq!(conf.eni.availability_zone, "us-west1");
    }

    // ---- TestReadCNIConfENIv2WithPlugins ----

    /// Conflist with ENI + IPAM pre-allocate inside the cilium-cni plugin.
    /// Mirrors TestReadCNIConfENIv2WithPlugins.
    #[test]
    fn parity_read_cni_conf_eni_v2_with_plugins() {
        let json = r#"
{
  "cniVersion":"0.3.1",
  "name":"cilium",
  "plugins": [
    {
      "cniVersion":"0.3.1",
      "type":"cilium-cni",
      "eni": {
        "first-interface-index":1,
        "security-groups":["sg-xxx"],
        "subnet-ids":["subnet-xxx"],
        "subnet-tags":{"foo":"true"},
        "exclude-interface-tags":{"bar":"false"}
      },
      "ipam": {"pre-allocate":5}
    }
  ]
}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.cni_version, "0.3.1");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.eni.first_interface_index, Some(1));
        assert_eq!(conf.eni.security_groups, vec!["sg-xxx"]);
        assert_eq!(conf.eni.subnet_ids, vec!["subnet-xxx"]);
        assert_eq!(
            conf.eni.subnet_tags.get("foo").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            conf.eni
                .exclude_interface_tags
                .get("bar")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(conf.ipam.pre_allocate, 5);
    }

    // ---- TestReadCNIConfAzurev2WithPlugins ----

    /// Conflist with Azure interface-name + IPAM pre-allocate.
    /// Mirrors TestReadCNIConfAzurev2WithPlugins.
    #[test]
    fn parity_read_cni_conf_azure_v2_with_plugins() {
        let json = r#"
{
  "cniVersion":"0.3.1",
  "name":"cilium",
  "plugins": [
    {
      "cniVersion":"0.3.1",
      "type":"cilium-cni",
      "azure": {"interface-name":"eth1"},
      "ipam": {"pre-allocate":5}
    }
  ]
}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.cni_version, "0.3.1");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.azure.interface_name, "eth1");
        assert_eq!(conf.ipam.pre_allocate, 5);
    }

    // ---- TestReadCNIConfIPAMType ----

    /// Conflist with IPAM type = "delegated-ipam".
    /// Mirrors TestReadCNIConfIPAMType.
    #[test]
    fn parity_read_cni_conf_ipam_type() {
        let json = r#"
{
  "cniVersion":"0.3.1",
  "name":"cilium",
  "plugins": [
    {
      "cniVersion":"0.3.1",
      "type":"cilium-cni",
      "ipam": {"type":"delegated-ipam"}
    }
  ]
}"#;
        let conf = parse_conf(json).expect("parse");
        assert_eq!(conf.cni_version, "0.3.1");
        assert_eq!(conf.plugin_type, "cilium-cni");
        assert_eq!(conf.ipam.ipam_type, "delegated-ipam");
    }

    // ---- TestReadCNIConfError ----

    /// MTU provided as a string instead of int must cause a parse error.
    /// Mirrors TestReadCNIConfError.
    #[test]
    fn parity_read_cni_conf_mtu_type_error() {
        let json = r#"{"name":"cilium","type":"cilium-cni","mtu":"9000"}"#;
        let result = parse_conf(json);
        assert!(result.is_err(), "expected parse error for string mtu");
    }

    static NEXT_DELETION_ID: AtomicU64 = AtomicU64::new(1);

    struct DeletionTestDir {
        path: PathBuf,
    }

    impl DeletionTestDir {
        fn new() -> Self {
            let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts");
            std::fs::create_dir_all(&base).expect("create cni .test-artifacts");

            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let id = NEXT_DELETION_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("cni-{nanos}-{id}"));
            std::fs::create_dir_all(&path).expect("create cni test dir");

            Self { path }
        }
    }

    impl Drop for DeletionTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum ErrorCase {
        Never,
        Once,
        Twice,
        Always,
    }

    #[derive(Debug, Clone)]
    struct ErrorMock {
        error_case: ErrorCase,
        err: DeletionError,
        call_count: usize,
    }

    impl ErrorMock {
        fn call(&mut self) -> DeletionResult<()> {
            self.call_count += 1;
            let should_fail = match self.error_case {
                ErrorCase::Never => false,
                ErrorCase::Once => self.call_count == 1,
                ErrorCase::Twice => self.call_count <= 2,
                ErrorCase::Always => true,
            };
            if should_fail {
                Err(self.err.clone())
            } else {
                Ok(())
            }
        }
    }

    struct FakeCiliumClient {
        error_mock: ErrorMock,
    }

    impl EndpointDeletionClient for FakeCiliumClient {
        fn endpoint_delete_many(
            &mut self,
            _request: &EndpointBatchDeleteRequest,
        ) -> DeletionResult<()> {
            self.error_mock.call()
        }
    }

    #[derive(Debug, Clone)]
    struct FakeCiliumClientCreator {
        error_mock: ErrorMock,
        client_error_mock: ErrorMock,
    }

    impl FakeCiliumClientCreator {
        fn describe(&self) -> String {
            fn label(error_case: ErrorCase) -> &'static str {
                match error_case {
                    ErrorCase::Never => "Never",
                    ErrorCase::Once => "Once",
                    ErrorCase::Twice => "Twice",
                    ErrorCase::Always => "Always",
                }
            }

            format!(
                "NewClient: {}, EndpointDelete: {}",
                label(self.error_mock.error_case),
                label(self.client_error_mock.error_case)
            )
        }

        fn new_client(&mut self) -> DeletionResult<Box<dyn EndpointDeletionClient>> {
            self.error_mock.call()?;
            Ok(Box::new(FakeCiliumClient {
                error_mock: self.client_error_mock.clone(),
            }))
        }
    }

    // ---- deletion_queue_test.go ----

    #[test]
    fn parity_test_deletion_fallback_client() {
        let test_cases = vec![
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::Client(String::from("unused")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                false,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::Client(String::from("unused")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                false,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::Client(String::from("unused")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Always,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                true,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::Client(String::from("error creating cilium client")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                false,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::Client(String::from("error creating cilium client")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                true,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::Client(String::from("error creating cilium client")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Twice,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                true,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Once,
                        err: DeletionError::Client(String::from("error creating cilium client")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Always,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                true,
            ),
            (
                FakeCiliumClientCreator {
                    error_mock: ErrorMock {
                        error_case: ErrorCase::Always,
                        err: DeletionError::Client(String::from("error creating cilium client")),
                        call_count: 0,
                    },
                    client_error_mock: ErrorMock {
                        error_case: ErrorCase::Never,
                        err: DeletionError::ServiceUnavailable,
                        call_count: 0,
                    },
                },
                true,
            ),
        ];

        let delete_request = EndpointBatchDeleteRequest {
            container_id: String::from("test-container-id"),
        };

        for (creator_state, should_queue_deletion) in test_cases {
            let test_dir = DeletionTestDir::new();
            let description = creator_state.describe();
            let queued_file = test_dir
                .path
                .join(deletion_request_filename(&delete_request).expect("queue filename"));
            let creator = Arc::new(Mutex::new(creator_state));
            let creator_for_client = Arc::clone(&creator);
            let mut deletion_client = DeletionFallbackClient::new_for_test(
                test_dir.path.clone(),
                Box::new(move |_| lock(&creator_for_client).new_client()),
            );

            let result = deletion_client.endpoint_delete_many(&delete_request);
            assert!(result.is_ok(), "{description}: {result:?}");
            assert_eq!(
                queued_file.exists(),
                should_queue_deletion,
                "{description}: queued file mismatch"
            );
        }
    }

    #[test]
    fn parity_test_queue_lock() {
        let test_dir = DeletionTestDir::new();
        let lock_dir = test_dir.path.join("deletion_queue");
        let mut deletion_client = DeletionFallbackClient::new_for_test(
            lock_dir.clone(),
            Box::new(|_| Err(DeletionError::Client(String::from("unused")))),
        );
        deletion_client.delete_queue_lockfile = lock_dir.join("lockfile");

        let lock_one = deletion_client
            .try_queue_lock()
            .expect("first shared lock should succeed");
        let lock_two = deletion_client
            .try_queue_lock()
            .expect("second shared lock should succeed");

        let exclusive = LockFile::new(&deletion_client.delete_queue_lockfile)
            .expect("exclusive lockfile should open");
        assert!(exclusive.lock_exclusive().is_err());

        lock_one.unlock();
        lock_two.unlock();

        let exclusive = LockFile::new(&deletion_client.delete_queue_lockfile)
            .expect("exclusive lockfile should reopen");
        assert!(exclusive.lock_exclusive().is_ok());
        exclusive.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.cni.identity.id, SecurityIdentity::unmanaged());
    }

    #[test]
    fn validate_rejects_empty_plugin_name() {
        let config = CniConfig::new("", "10.42.0.0/24".parse().expect("valid cni pod cidr"));

        let error = config.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Cni(_)));
    }

    #[test]
    fn report_roundtrips_through_json() {
        let json = serde_json::to_string(&scaffold()).expect("report serializes");
        let decoded: CniReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.component, COMPONENT);
        assert!(decoded.ready);
    }
}

#[cfg(test)]
mod pure_type_tests {
    use super::pure::*;

    #[test]
    fn test_cni_command_parse() {
        assert_eq!("ADD".parse::<CNICommand>().unwrap(), CNICommand::Add);
        assert_eq!("DEL".parse::<CNICommand>().unwrap(), CNICommand::Del);
        assert!("ATTACH".parse::<CNICommand>().is_err());
    }

    #[test]
    fn test_cni_args_extra_parse() {
        let args = CNIArgs {
            command: CNICommand::Add,
            container_id: "abc".into(),
            netns: "/proc/1/ns/net".into(),
            ifname: "eth0".into(),
            args: "K8S_POD_NAME=nginx;K8S_POD_NAMESPACE=default".into(),
            path: vec![],
        };
        let map = args.parse_extra_args();
        assert_eq!(map.get("K8S_POD_NAME").map(String::as_str), Some("nginx"));
        assert_eq!(
            map.get("K8S_POD_NAMESPACE").map(String::as_str),
            Some("default")
        );
    }

    #[test]
    fn test_net_conf_serde() {
        let conf = NetConf {
            cni_version: "0.4.0".into(),
            name: "cilium".into(),
            plugin_type: "cilium-cni".into(),
            ipam: None,
            dns: None,
            enable_debug: false,
        };
        let json = serde_json::to_string(&conf).unwrap();
        let back: NetConf = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "cilium");
        assert_eq!(back.plugin_type, "cilium-cni");
    }

    #[test]
    fn test_cni_result_builder() {
        let mut result = CNIResult::new("1.0.0");
        result.ips.push(IPConfig {
            address: "10.0.0.5/24".parse().unwrap(),
            gateway: Some("10.0.0.1".parse().unwrap()),
            interface_index: Some(0),
        });
        assert_eq!(result.ips.len(), 1);
        assert_eq!(result.cni_version, "1.0.0");
    }

    #[test]
    fn test_dns_config_serde() {
        let dns = DNSConfig {
            nameservers: vec!["8.8.8.8".into()],
            domain: "cluster.local".into(),
            search: vec!["default.svc.cluster.local".into()],
            options: vec![],
        };
        let json = serde_json::to_string(&dns).unwrap();
        let back: DNSConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nameservers, vec!["8.8.8.8"]);
    }
}
