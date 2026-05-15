//! CNI plugin execution: handles ADD, DEL, CHECK, VERSION calls from the container runtime.
//!
//! The CNI spec requires the plugin to read config from stdin and return results on stdout.
//! Env vars CNI_COMMAND, CNI_CONTAINERID, CNI_NETNS, CNI_IFNAME, CNI_ARGS, CNI_PATH carry context.

use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;
use std::process::Command;
use std::time::Duration;

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

const DEFAULT_CILIUM_SOCK_PATH: &str = "/var/run/cilium/cilium.sock";
const CILIUM_SOCK_PATH_ENV: &str = "CILIUM_SOCK_PATH";

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

impl PluginError {
    fn is_ip_file_exists(&self) -> bool {
        matches!(self, Self::SetupFailed(message) if message.contains("RTNETLINK answers: File exists"))
    }
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

#[derive(Debug)]
struct AddAddressing {
    gateway: String,
    pod_cidr: String,
}

#[derive(Debug, Deserialize)]
struct AgentIpamResponse {
    #[serde(default)]
    ipv4: Option<AgentIpv4Addressing>,
    #[serde(default, rename = "host-addressing")]
    host_addressing: Option<AgentHostAddressing>,
}

#[derive(Debug, Deserialize)]
struct AgentHostAddressing {
    #[serde(default)]
    ipv4: Option<AgentHostIpv4Addressing>,
}

#[derive(Debug, Deserialize)]
struct AgentHostIpv4Addressing {
    #[serde(default)]
    ip: String,
    #[serde(default, rename = "alloc-range")]
    alloc_range: String,
}

#[derive(Debug, Deserialize)]
struct AgentIpv4Addressing {
    #[serde(default)]
    ip: String,
    #[serde(default)]
    gateway: String,
    #[serde(default)]
    cidrs: Vec<String>,
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
        CniCommand::Add => run_add(ctx, None),
        CniCommand::Del => {
            info!(container = %ctx.container_id, netns = %ctx.netns, "CNI DEL");
            if !ctx.container_id.is_empty() {
                let host_if = host_ifname(&ctx.container_id);
                let _ = run_ip(&["link", "del", &host_if]);
            }
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

fn run_add(ctx: &CniContext, agent_sock_path: Option<&str>) -> Result<String, PluginError> {
    info!(container = %ctx.container_id, netns = %ctx.netns, ifname = %ctx.ifname, "CNI ADD");
    let AddAddressing { gateway, pod_cidr } = resolve_add_addressing(ctx, agent_sock_path);
    let gateway_route = gateway_route_cidr(&gateway)?;

    if !ctx.netns.trim().is_empty() {
        let host_if = host_ifname(&ctx.container_id);
        setup_veth_pair(
            &ctx.netns,
            &host_if,
            &ctx.ifname,
            &gateway,
            &gateway_route,
            &pod_cidr,
        )?;
    }

    let result = CniAddResult {
        cni_version: "1.0.0".to_string(),
        interfaces: vec![serde_json::json!({
            "name": ctx.ifname,
            "sandbox": ctx.netns,
        })],
        ips: vec![serde_json::json!({
            "interface": 0,
            "address": pod_cidr,
            "gateway": gateway,
        })],
        routes: vec![
            serde_json::json!({
                "dst": gateway_route,
            }),
            serde_json::json!({
                "dst": "0.0.0.0/0",
                "gw": gateway,
            }),
        ],
        dns: serde_json::json!({}),
    };
    Ok(serde_json::to_string(&result)?)
}

fn resolve_add_addressing(ctx: &CniContext, agent_sock_path: Option<&str>) -> AddAddressing {
    match allocate_add_addressing_from_agent(agent_sock_path) {
        Ok(addressing) => addressing,
        Err(error) => {
            warn!(error = %error, "unable to allocate addressing from cilium agent; using fallback addressing");
            fallback_add_addressing(ctx)
        }
    }
}

fn fallback_add_addressing(ctx: &CniContext) -> AddAddressing {
    let host_octet = ctx.container_id.bytes().fold(17_u8, u8::wrapping_add);
    let pod_octet = if host_octet == u8::MAX {
        250
    } else {
        host_octet.saturating_add(1)
    };

    AddAddressing {
        gateway: format!("10.244.255.{host_octet}"),
        pod_cidr: format!("10.244.255.{pod_octet}/32"),
    }
}

#[cfg(unix)]
fn allocate_add_addressing_from_agent(
    agent_sock_path: Option<&str>,
) -> Result<AddAddressing, PluginError> {
    let sock_path = agent_sock_path
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var(CILIUM_SOCK_PATH_ENV)
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_CILIUM_SOCK_PATH.to_string());
    allocate_add_addressing_from_agent_at_path(&sock_path)
}

#[cfg(unix)]
fn allocate_add_addressing_from_agent_at_path(
    sock_path: &str,
) -> Result<AddAddressing, PluginError> {
    use std::io::Write;

    let mut stream = StdUnixStream::connect(sock_path)
        .map_err(|error| PluginError::SetupFailed(format!("connect {sock_path}: {error}")))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| PluginError::SetupFailed(format!("set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| PluginError::SetupFailed(format!("set write timeout: {error}")))?;

    stream
        .write_all(b"POST /ipam HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .map_err(|error| PluginError::SetupFailed(format!("write /ipam request: {error}")))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| PluginError::SetupFailed(format!("read /ipam response: {error}")))?;

    parse_agent_ipam_response(&response)
}

#[cfg(not(unix))]
fn allocate_add_addressing_from_agent(
    _agent_sock_path: Option<&str>,
) -> Result<AddAddressing, PluginError> {
    Err(PluginError::SetupFailed(
        "cilium agent unix socket IPAM is unsupported on this platform".to_string(),
    ))
}

fn parse_agent_ipam_response(response: &str) -> Result<AddAddressing, PluginError> {
    let mut status_line = response.lines();
    let status_code = status_line
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            PluginError::SetupFailed("invalid cilium agent HTTP response".to_string())
        })?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| {
            PluginError::SetupFailed("missing cilium agent response body".to_string())
        })?;

    if status_code != 200 && status_code != 201 {
        return Err(PluginError::SetupFailed(format!(
            "cilium agent /ipam returned HTTP {status_code}: {body}"
        )));
    }

    let parsed: AgentIpamResponse = serde_json::from_str(body)?;
    let ipv4 = parsed.ipv4.ok_or_else(|| {
        PluginError::SetupFailed("cilium agent /ipam missing ipv4 block".to_string())
    })?;
    let gateway = if ipv4.gateway.is_empty() {
        parsed
            .host_addressing
            .as_ref()
            .and_then(|host| host.ipv4.as_ref())
            .map(|host| host.ip.clone())
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| {
                PluginError::SetupFailed("cilium agent /ipam missing gateway".to_string())
            })?
    } else {
        ipv4.gateway
    };
    let alloc_cidr = ipv4
        .cidrs
        .first()
        .cloned()
        .or_else(|| {
            parsed
                .host_addressing
                .as_ref()
                .and_then(|host| host.ipv4.as_ref())
                .map(|host| host.alloc_range.clone())
                .filter(|cidr| !cidr.is_empty())
        })
        .ok_or_else(|| PluginError::SetupFailed("cilium agent /ipam missing CIDR".to_string()))?;
    let pod_cidr = format_ip_with_prefix(&ipv4.ip, &alloc_cidr)?;

    Ok(AddAddressing { gateway, pod_cidr })
}

fn format_ip_with_prefix(ip: &str, alloc_cidr: &str) -> Result<String, PluginError> {
    let address = ip.parse::<std::net::Ipv4Addr>().map_err(|error| {
        PluginError::SetupFailed(format!("invalid IP from cilium agent /ipam: {error}"))
    })?;
    let _ = alloc_cidr.parse::<Ipv4Net>().map_err(|error| {
        PluginError::SetupFailed(format!("invalid CIDR from cilium agent /ipam: {error}"))
    })?;

    Ok(format!("{address}/32"))
}

fn gateway_route_cidr(gateway: &str) -> Result<String, PluginError> {
    let gateway = gateway.parse::<std::net::Ipv4Addr>().map_err(|error| {
        PluginError::SetupFailed(format!(
            "invalid gateway IP from cilium agent /ipam: {error}"
        ))
    })?;
    Ok(format!("{gateway}/32"))
}

fn host_ifname(container_id: &str) -> String {
    let suffix: String = container_id
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .collect();
    format!("lxc{suffix}")
}

fn peer_ifname(host_if: &str) -> String {
    let suffix = host_if.strip_prefix("lxc").unwrap_or(host_if);
    format!("tmp{suffix}")
}

trait NetOps {
    fn run_ip(&mut self, args: &[&str]) -> Result<(), PluginError>;
    fn run_nsenter(&mut self, netns: &str, args: &[&str]) -> Result<(), PluginError>;
    fn link_exists(&mut self, link: &str) -> Result<bool, PluginError>;
    fn write_sysctl(&mut self, path: &str, value: &str) -> Result<(), PluginError>;
}

struct RealNetOps;

impl NetOps for RealNetOps {
    fn run_ip(&mut self, args: &[&str]) -> Result<(), PluginError> {
        run_ip(args)
    }

    fn run_nsenter(&mut self, netns: &str, args: &[&str]) -> Result<(), PluginError> {
        run_nsenter(netns, args)
    }

    fn link_exists(&mut self, link: &str) -> Result<bool, PluginError> {
        link_exists(link)
    }

    fn write_sysctl(&mut self, path: &str, value: &str) -> Result<(), PluginError> {
        write_sysctl(path, value)
    }
}

fn setup_veth_pair(
    netns: &str,
    host_if: &str,
    ifname: &str,
    gateway: &str,
    gateway_route: &str,
    pod_cidr: &str,
) -> Result<(), PluginError> {
    let mut ops = RealNetOps;
    setup_veth_pair_with_ops(
        &mut ops,
        netns,
        host_if,
        ifname,
        gateway,
        gateway_route,
        pod_cidr,
    )?;
    Ok(())
}

fn setup_veth_pair_with_ops(
    ops: &mut impl NetOps,
    netns: &str,
    host_if: &str,
    ifname: &str,
    gateway: &str,
    gateway_route: &str,
    pod_cidr: &str,
) -> Result<(), PluginError> {
    let pod_ip = pod_ip_from_cidr(pod_cidr)?;
    let pod_route = format!("{pod_ip}/32");
    let peer_if = peer_ifname(host_if);
    let host_proxy_arp = format!("/proc/sys/net/ipv4/conf/{host_if}/proxy_arp");
    let host_forwarding = format!("/proc/sys/net/ipv4/conf/{host_if}/forwarding");

    ensure_host_gateway_with_ops(ops, gateway)?;
    let _ = ops.run_ip(&["link", "del", host_if]);
    ops.run_ip(&[
        "link", "add", host_if, "type", "veth", "peer", "name", &peer_if,
    ])?;
    ops.run_ip(&["link", "set", &peer_if, "netns", netns])?;
    ops.run_ip(&["link", "set", host_if, "up"])?;
    ops.write_sysctl(&host_proxy_arp, "1")?;
    ops.write_sysctl(&host_forwarding, "1")?;
    ops.run_ip(&["route", "replace", &pod_route, "dev", host_if])?;

    ops.run_nsenter(netns, &["ip", "link", "set", "lo", "up"])?;
    ops.run_nsenter(netns, &["ip", "link", "set", &peer_if, "name", ifname])?;
    ops.run_nsenter(netns, &["ip", "link", "set", ifname, "up"])?;
    ops.run_nsenter(netns, &["ip", "addr", "add", pod_cidr, "dev", ifname])?;
    let _ = ops.run_nsenter(netns, &["ip", "route", "del", gateway_route]);
    ops.run_nsenter(
        netns,
        &["ip", "route", "replace", gateway_route, "dev", ifname],
    )?;
    let _ = ops.run_nsenter(netns, &["ip", "route", "del", "default"]);
    ops.run_nsenter(
        netns,
        &[
            "ip", "route", "replace", "default", "via", gateway, "dev", ifname,
        ],
    )?;
    Ok(())
}

fn ensure_host_gateway_with_ops(ops: &mut impl NetOps, gateway: &str) -> Result<(), PluginError> {
    let host_exists = ops.link_exists("cilium_host")?;
    let net_exists = ops.link_exists("cilium_net")?;

    match (host_exists, net_exists) {
        (true, true) => {}
        (true, false) => {
            let _ = ops.run_ip(&["link", "del", "cilium_host"]);
            ops.run_ip(&[
                "link",
                "add",
                "cilium_host",
                "type",
                "veth",
                "peer",
                "name",
                "cilium_net",
            ])?;
        }
        (false, true) => {
            let _ = ops.run_ip(&["link", "del", "cilium_net"]);
            ops.run_ip(&[
                "link",
                "add",
                "cilium_host",
                "type",
                "veth",
                "peer",
                "name",
                "cilium_net",
            ])?;
        }
        (false, false) => {
            let result = ops.run_ip(&[
                "link",
                "add",
                "cilium_host",
                "type",
                "veth",
                "peer",
                "name",
                "cilium_net",
            ]);
            if let Err(error) = result {
                if !(error.is_ip_file_exists()
                    && ops.link_exists("cilium_host")?
                    && ops.link_exists("cilium_net")?)
                {
                    return Err(error);
                }
            }
        }
    }

    let gateway_with_prefix = format!("{gateway}/32");
    ops.run_ip(&["link", "set", "cilium_host", "up"])?;
    ops.run_ip(&["link", "set", "cilium_net", "up"])?;
    ops.run_ip(&[
        "addr",
        "replace",
        &gateway_with_prefix,
        "dev",
        "cilium_host",
    ])?;
    Ok(())
}

fn pod_ip_from_cidr(pod_cidr: &str) -> Result<std::net::Ipv4Addr, PluginError> {
    pod_cidr
        .parse::<Ipv4Net>()
        .map(|cidr| cidr.addr())
        .map_err(|error| PluginError::SetupFailed(format!("invalid pod CIDR {pod_cidr}: {error}")))
}

fn run_ip(args: &[&str]) -> Result<(), PluginError> {
    let output = Command::new("ip")
        .args(args)
        .output()
        .map_err(|error| PluginError::SetupFailed(error.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(PluginError::SetupFailed(format!(
        "ip command failed: {} ({stderr})",
        output.status
    )))
}

fn link_exists(link: &str) -> Result<bool, PluginError> {
    let output = Command::new("ip")
        .args(["link", "show", "dev", link])
        .output()
        .map_err(|error| PluginError::SetupFailed(error.to_string()))?;
    Ok(output.status.success())
}

fn write_sysctl(path: &str, value: &str) -> Result<(), PluginError> {
    std::fs::write(path, value)
        .map_err(|error| PluginError::SetupFailed(format!("write sysctl {path}={value}: {error}")))
}

fn run_nsenter(netns: &str, args: &[&str]) -> Result<(), PluginError> {
    let mut full_args = vec![format!("--net={netns}")];
    full_args.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = Command::new("nsenter")
        .args(full_args)
        .output()
        .map_err(|error| PluginError::SetupFailed(error.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(PluginError::SetupFailed(format!(
        "nsenter command failed: {} ({stderr})",
        output.status
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Write as _;
    use std::os::unix::net::UnixListener;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeNetOps {
        existing_links: BTreeSet<String>,
        ip_calls: Vec<Vec<String>>,
        nsenter_calls: Vec<(String, Vec<String>)>,
        sysctl_writes: Vec<(String, String)>,
        fail_ip_calls: Vec<(Vec<String>, PluginError)>,
    }

    impl FakeNetOps {
        fn with_existing_links(links: &[&str]) -> Self {
            Self {
                existing_links: links.iter().map(|link| (*link).to_string()).collect(),
                ..Self::default()
            }
        }

        fn with_failed_ip_call(links: &[&str], args: &[&str], error: PluginError) -> Self {
            Self {
                existing_links: links.iter().map(|link| (*link).to_string()).collect(),
                fail_ip_calls: vec![(args.iter().map(|arg| (*arg).to_string()).collect(), error)],
                ..Self::default()
            }
        }
    }

    impl NetOps for FakeNetOps {
        fn run_ip(&mut self, args: &[&str]) -> Result<(), PluginError> {
            let call: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
            self.ip_calls.push(call.clone());
            if let Some(index) = self
                .fail_ip_calls
                .iter()
                .position(|(expected, _)| expected == &call)
            {
                let (_expected, error) = self.fail_ip_calls.remove(index);
                if call
                    == vec![
                        "link".to_string(),
                        "add".to_string(),
                        "cilium_host".to_string(),
                        "type".to_string(),
                        "veth".to_string(),
                        "peer".to_string(),
                        "name".to_string(),
                        "cilium_net".to_string(),
                    ]
                    && error.is_ip_file_exists()
                {
                    self.existing_links.insert("cilium_host".to_string());
                    self.existing_links.insert("cilium_net".to_string());
                }
                return Err(error);
            }
            Ok(())
        }

        fn run_nsenter(&mut self, netns: &str, args: &[&str]) -> Result<(), PluginError> {
            self.nsenter_calls.push((
                netns.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect(),
            ));
            Ok(())
        }

        fn link_exists(&mut self, link: &str) -> Result<bool, PluginError> {
            Ok(self.existing_links.contains(link))
        }

        fn write_sysctl(&mut self, path: &str, value: &str) -> Result<(), PluginError> {
            self.sysctl_writes
                .push((path.to_string(), value.to_string()));
            Ok(())
        }
    }

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
    fn test_host_ifname_uses_container_hash_prefix() {
        assert_eq!(host_ifname("abc123"), "lxcabc123");
    }

    #[test]
    fn test_peer_ifname_avoids_host_eth0_collision() {
        assert_eq!(peer_ifname("lxctest123"), "tmptest123");
    }

    #[test]
    fn test_setup_veth_pair_creates_host_gateway_and_host_routes() {
        let mut ops = FakeNetOps::default();

        setup_veth_pair_with_ops(
            &mut ops,
            "/proc/123/ns/net",
            "lxctest123",
            "eth0",
            "10.244.7.1",
            "10.244.7.1/32",
            "10.244.7.23/32",
        )
        .expect("veth setup should succeed");

        assert!(ops.ip_calls.contains(&vec![
            "link".to_string(),
            "add".to_string(),
            "cilium_host".to_string(),
            "type".to_string(),
            "veth".to_string(),
            "peer".to_string(),
            "name".to_string(),
            "cilium_net".to_string(),
        ]));
        assert!(ops.ip_calls.contains(&vec![
            "addr".to_string(),
            "replace".to_string(),
            "10.244.7.1/32".to_string(),
            "dev".to_string(),
            "cilium_host".to_string(),
        ]));
        assert!(ops.ip_calls.contains(&vec![
            "route".to_string(),
            "replace".to_string(),
            "10.244.7.23/32".to_string(),
            "dev".to_string(),
            "lxctest123".to_string(),
        ]));
        assert!(ops.ip_calls.contains(&vec![
            "link".to_string(),
            "add".to_string(),
            "lxctest123".to_string(),
            "type".to_string(),
            "veth".to_string(),
            "peer".to_string(),
            "name".to_string(),
            "tmptest123".to_string(),
        ]));
        assert!(ops.nsenter_calls.contains(&(
            "/proc/123/ns/net".to_string(),
            vec![
                "ip".to_string(),
                "link".to_string(),
                "set".to_string(),
                "tmptest123".to_string(),
                "name".to_string(),
                "eth0".to_string(),
            ],
        )));
        assert_eq!(
            ops.sysctl_writes,
            vec![
                (
                    "/proc/sys/net/ipv4/conf/lxctest123/proxy_arp".to_string(),
                    "1".to_string(),
                ),
                (
                    "/proc/sys/net/ipv4/conf/lxctest123/forwarding".to_string(),
                    "1".to_string(),
                ),
            ]
        );
        assert!(
            !ops.ip_calls.iter().any(|call| call
                == &vec![
                    "addr".to_string(),
                    "add".to_string(),
                    "10.244.7.1/24".to_string(),
                    "dev".to_string(),
                    "lxctest123".to_string(),
                ]),
            "host-side veth must not own the gateway IP",
        );
    }

    #[test]
    fn test_setup_veth_pair_reuses_existing_host_gateway_pair() {
        let mut ops = FakeNetOps::with_existing_links(&["cilium_host", "cilium_net"]);

        setup_veth_pair_with_ops(
            &mut ops,
            "/proc/123/ns/net",
            "lxctest123",
            "eth0",
            "10.244.7.1",
            "10.244.7.1/32",
            "10.244.7.23/32",
        )
        .expect("veth setup should succeed");

        assert!(
            !ops.ip_calls.iter().any(|call| call
                == &vec![
                    "link".to_string(),
                    "add".to_string(),
                    "cilium_host".to_string(),
                    "type".to_string(),
                    "veth".to_string(),
                    "peer".to_string(),
                    "name".to_string(),
                    "cilium_net".to_string(),
                ]),
            "existing cilium_host/cilium_net pair should be reused",
        );
        assert!(ops.ip_calls.contains(&vec![
            "addr".to_string(),
            "replace".to_string(),
            "10.244.7.1/32".to_string(),
            "dev".to_string(),
            "cilium_host".to_string(),
        ]));
    }

    #[test]
    fn test_setup_veth_pair_tolerates_concurrent_host_gateway_creation() {
        let mut ops = FakeNetOps::with_failed_ip_call(
            &[],
            &[
                "link",
                "add",
                "cilium_host",
                "type",
                "veth",
                "peer",
                "name",
                "cilium_net",
            ],
            PluginError::SetupFailed(
                "ip command failed: exit status: 2 (RTNETLINK answers: File exists\n)".to_string(),
            ),
        );

        setup_veth_pair_with_ops(
            &mut ops,
            "/proc/123/ns/net",
            "lxctest123",
            "eth0",
            "10.244.7.1",
            "10.244.7.1/32",
            "10.244.7.23/32",
        )
        .expect("veth setup should reuse the concurrently created host gateway pair");

        assert!(ops.existing_links.contains("cilium_host"));
        assert!(ops.existing_links.contains("cilium_net"));
        assert!(ops.ip_calls.contains(&vec![
            "addr".to_string(),
            "replace".to_string(),
            "10.244.7.1/32".to_string(),
            "dev".to_string(),
            "cilium_host".to_string(),
        ]));
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
            netns: String::new(),
            ifname: "eth0".to_string(),
            args: String::new(),
            path: "/opt/cni/bin".to_string(),
            stdin_data: vec![],
        };

        let result = run(&ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["cniVersion"], "1.0.0");
        assert_eq!(parsed["interfaces"][0]["name"], "eth0");
        assert!(
            parsed["ips"][0]["address"]
                .as_str()
                .unwrap()
                .starts_with("10.244.255.")
        );
    }

    #[test]
    fn test_add_response_uses_agent_ipam_when_available() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::current_dir()
            .expect("current dir should resolve")
            .join("target")
            .join(format!("seriousum-cni-test-{uniq}"));
        std::fs::create_dir_all(&root).expect("test directory should be created");
        let sock_path = root.join("cilium.sock");
        let listener = UnixListener::bind(&sock_path).expect("test socket should bind");

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client should connect");
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buf).expect("request should be readable");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).expect("request should be valid UTF-8");
            assert!(request.starts_with("POST /ipam HTTP/1.1"));

            let body = serde_json::json!({
                "host-addressing": {
                    "ipv4": {
                        "enabled": true,
                        "ip": "10.244.7.1",
                        "alloc-range": "10.244.7.0/24",
                    }
                },
                "ipv4": {
                    "ip": "10.244.7.23",
                    "gateway": "10.244.7.1",
                    "cidrs": ["10.244.7.0/24"],
                    "interface-number": "0",
                }
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should be written");
        });

        let ctx = CniContext {
            command: CniCommand::Add,
            container_id: "c123".to_string(),
            netns: String::new(),
            ifname: "eth0".to_string(),
            args: String::new(),
            path: "/opt/cni/bin".to_string(),
            stdin_data: vec![],
        };

        let result =
            run_add(&ctx, Some(sock_path.to_string_lossy().as_ref())).expect("add should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("result should be JSON");
        assert_eq!(parsed["ips"][0]["address"], "10.244.7.23/32");
        assert_eq!(parsed["ips"][0]["gateway"], "10.244.7.1");
        assert_eq!(parsed["routes"][0]["dst"], "10.244.7.1/32");
        assert_eq!(parsed["routes"][1]["gw"], "10.244.7.1");
        server.join().expect("server thread should finish");
        std::fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn test_parse_agent_ipam_response_uses_host_gateway_prefix_length() {
        let body = serde_json::json!({
            "host-addressing": {
                "ipv4": {
                    "enabled": true,
                    "ip": "10.244.7.1",
                    "alloc-range": "10.244.7.0/24",
                }
            },
            "ipv4": {
                "ip": "10.244.7.23",
                "gateway": "10.244.7.1",
                "cidrs": ["10.244.7.0/24"],
                "interface-number": "0",
            }
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        let parsed = parse_agent_ipam_response(&response).expect("response should parse");

        assert_eq!(parsed.gateway, "10.244.7.1");
        assert_eq!(parsed.pod_cidr, "10.244.7.23/32");
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
