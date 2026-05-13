//! CNI plugin execution: handles ADD, DEL, CHECK, VERSION calls from the container runtime.
//!
//! The CNI spec requires the plugin to read config from stdin and return results on stdout.
//! Env vars CNI_COMMAND, CNI_CONTAINERID, CNI_NETNS, CNI_IFNAME, CNI_ARGS, CNI_PATH carry context.

use std::io::{self, Read};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

/// CNI error codes (from CNI spec).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CniErrorResult {
    /// Result CNI version.
    pub cni_version: String,
    /// Numeric CNI error code.
    pub code: u32,
    /// Short error message.
    pub msg: String,
    /// Detailed error description.
    pub details: String,
}

/// Errors returned while dispatching CNI plugin commands.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Standard input or environment I/O failed.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// JSON parsing or serialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// `CNI_COMMAND` was not present in the environment.
    #[error("missing CNI_COMMAND env var")]
    MissingCommand,
    /// The runtime requested an unsupported command.
    #[error("unsupported CNI command: {0}")]
    UnsupportedCommand(String),
    /// Network setup failed.
    #[error("network setup failed: {0}")]
    SetupFailed(String),
}

/// CNI command from environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CniCommand {
    /// Add networking for a container.
    Add,
    /// Delete networking for a container.
    Del,
    /// Check existing networking state.
    Check,
    /// Report supported CNI versions.
    Version,
}

impl std::str::FromStr for CniCommand {
    type Err = PluginError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ADD" => Ok(Self::Add),
            "DEL" => Ok(Self::Del),
            "CHECK" => Ok(Self::Check),
            "VERSION" => Ok(Self::Version),
            other => Err(PluginError::UnsupportedCommand(other.to_string())),
        }
    }
}

/// Context passed to each CNI command handler.
#[derive(Debug)]
pub struct CniContext {
    /// Requested CNI command.
    pub command: CniCommand,
    /// Container identifier.
    pub container_id: String,
    /// Target network namespace path.
    pub netns: String,
    /// Interface name inside the container.
    pub ifname: String,
    /// Raw `CNI_ARGS` value.
    pub args: String,
    /// Plugin search path from `CNI_PATH`.
    pub path: String,
    /// Raw stdin configuration payload.
    pub stdin_data: Vec<u8>,
}

impl CniContext {
    /// Build context from current environment and stdin.
    pub fn from_env() -> Result<Self, PluginError> {
        use std::env;

        let command: CniCommand = env::var("CNI_COMMAND")
            .map_err(|_| PluginError::MissingCommand)?
            .parse()?;

        Ok(Self {
            command,
            container_id: env::var("CNI_CONTAINERID").unwrap_or_default(),
            netns: env::var("CNI_NETNS").unwrap_or_default(),
            ifname: env::var("CNI_IFNAME").unwrap_or_default(),
            args: env::var("CNI_ARGS").unwrap_or_default(),
            path: env::var("CNI_PATH").unwrap_or_default(),
            stdin_data: {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)?;
                buf
            },
        })
    }
}

/// CNI VERSION response.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CniVersionResult {
    /// Negotiated CNI version.
    pub cni_version: String,
    /// Supported plugin versions.
    pub supported_versions: Vec<String>,
}

/// Minimal CNI result for ADD.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CniAddResult {
    /// Negotiated CNI version.
    pub cni_version: String,
    /// Reported interfaces.
    pub interfaces: Vec<serde_json::Value>,
    /// Assigned IPs.
    pub ips: Vec<serde_json::Value>,
    /// Installed routes.
    pub routes: Vec<serde_json::Value>,
    /// DNS settings.
    pub dns: serde_json::Value,
}

/// Dispatch the CNI command and write result to stdout.
pub fn run(ctx: &CniContext) -> Result<String, PluginError> {
    debug!(command = ?ctx.command, container = %ctx.container_id, "CNI dispatch");

    match ctx.command {
        CniCommand::Version => {
            let result = CniVersionResult {
                cni_version: "1.0.0".to_string(),
                supported_versions: vec![
                    "0.3.1".to_string(),
                    "0.4.0".to_string(),
                    "1.0.0".to_string(),
                ],
            };
            Ok(serde_json::to_string(&result)?)
        }
        CniCommand::Add => {
            info!(container = %ctx.container_id, netns = %ctx.netns, ifname = %ctx.ifname, "CNI ADD");
            // TODO(phase3): allocate IP via IPAM, configure veth pair, set up eBPF policy
            let result = CniAddResult {
                cni_version: "1.0.0".to_string(),
                interfaces: vec![],
                ips: vec![],
                routes: vec![],
                dns: serde_json::json!({}),
            };
            Ok(serde_json::to_string(&result)?)
        }
        CniCommand::Del => {
            info!(container = %ctx.container_id, netns = %ctx.netns, "CNI DEL");
            warn!(container = %ctx.container_id, "CNI DEL is a no-op scaffold");
            // TODO(phase3): release IP, tear down veth pair, remove eBPF policy
            Ok("{}".to_string())
        }
        CniCommand::Check => {
            debug!(container = %ctx.container_id, "CNI CHECK");
            warn!(container = %ctx.container_id, "CNI CHECK is a no-op scaffold");
            // TODO(phase3): verify network setup is correct
            Ok("{}".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cni_command_parse() {
        assert_eq!("ADD".parse::<CniCommand>().unwrap(), CniCommand::Add);
        assert_eq!("DEL".parse::<CniCommand>().unwrap(), CniCommand::Del);
        assert_eq!("CHECK".parse::<CniCommand>().unwrap(), CniCommand::Check);
        assert_eq!(
            "VERSION".parse::<CniCommand>().unwrap(),
            CniCommand::Version
        );
        assert!("INVALID".parse::<CniCommand>().is_err());
    }

    #[test]
    fn test_version_response() {
        let ctx = CniContext {
            command: CniCommand::Version,
            container_id: "test-container".to_string(),
            netns: "/proc/1/ns/net".to_string(),
            ifname: "eth0".to_string(),
            args: String::new(),
            path: "/opt/cni/bin".to_string(),
            stdin_data: vec![],
        };

        let result = run(&ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["cniVersion"], "1.0.0");
        assert!(parsed["supportedVersions"].is_array());
    }

    #[test]
    fn test_add_response() {
        let ctx = CniContext {
            command: CniCommand::Add,
            container_id: "c123".to_string(),
            netns: "/var/run/netns/cni-abc".to_string(),
            ifname: "eth0".to_string(),
            args: String::new(),
            path: "/opt/cni/bin".to_string(),
            stdin_data: vec![],
        };

        let result = run(&ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["cniVersion"], "1.0.0");
    }

    #[test]
    fn test_del_response() {
        let ctx = CniContext {
            command: CniCommand::Del,
            container_id: "c123".to_string(),
            netns: String::new(),
            ifname: "eth0".to_string(),
            args: String::new(),
            path: "/opt/cni/bin".to_string(),
            stdin_data: vec![],
        };

        let result = run(&ctx).unwrap();
        assert_eq!(result, "{}");
    }
}
