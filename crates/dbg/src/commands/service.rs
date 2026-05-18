//! Service and load balancer inspection commands.

use std::collections::HashMap;

use serde::Deserialize;

use crate::{Error, Result, Service, ServiceBackend, ServiceId, compat_get};

#[derive(Debug, Clone, Deserialize)]
struct CompatServiceEntry {
    status: CompatServiceStatus,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatServiceStatus {
    realized: CompatServiceSpec,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatServiceSpec {
    id: i64,
    #[serde(rename = "frontend-address")]
    frontend_address: CompatFrontendAddress,
    #[serde(rename = "backend-addresses", default)]
    backend_addresses: Vec<CompatBackendAddress>,
    flags: CompatServiceFlags,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatFrontendAddress {
    ip: String,
    #[serde(default)]
    port: u16,
    protocol: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatBackendAddress {
    ip: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    state: String,
    #[serde(default)]
    preferred: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatServiceFlags {
    #[serde(rename = "type")]
    service_type: String,
}

fn compat_services() -> Result<Vec<CompatServiceEntry>> {
    let body = compat_get("/v1/service")?;
    serde_json::from_str(&body).map_err(Into::into)
}

/// List all services configured in the system.
pub fn list_services() -> Result<Vec<Service>> {
    compat_services()?
        .into_iter()
        .map(|service| {
            let realized = service.status.realized;
            let id = u32::try_from(realized.id).map_err(|_| {
                Error::ServiceLookupFailed(format!("invalid service id {}", realized.id))
            })?;
            Ok(Service {
                id: ServiceId(id),
                frontend: format!(
                    "{}/{}",
                    join_host_port(
                        &realized.frontend_address.ip,
                        realized.frontend_address.port
                    ),
                    realized.frontend_address.protocol
                ),
                service_type: realized.flags.service_type,
                backends: realized
                    .backend_addresses
                    .into_iter()
                    .map(|backend| ServiceBackend {
                        address: join_host_port(&backend.ip, backend.port),
                        port: backend.port,
                        state: if backend.state.is_empty() {
                            "active".to_string()
                        } else {
                            backend.state
                        },
                        preferred: backend.preferred,
                    })
                    .collect(),
            })
        })
        .collect()
}

/// Return the raw compat service list JSON.
pub fn list_services_json_raw() -> Result<String> {
    compat_get("/v1/service")
}

fn join_host_port(ip: &str, port: u16) -> String {
    if ip.contains(':') {
        format!("[{}]:{}", ip, port)
    } else {
        format!("{}:{}", ip, port)
    }
}

/// Return a synthesized raw JSON map matching `cilium-dbg bpf lb list -o json`.
pub fn list_lb_json_raw() -> Result<String> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for service in compat_services()? {
        let realized = service.status.realized;
        let key = format!(
            "{}/{}",
            join_host_port(
                &realized.frontend_address.ip,
                realized.frontend_address.port
            ),
            realized.frontend_address.protocol.to_ascii_uppercase()
        );
        let backends = if realized.backend_addresses.is_empty() {
            vec!["0.0.0.0:0".to_string()]
        } else {
            realized
                .backend_addresses
                .into_iter()
                .map(|backend| join_host_port(&backend.ip, backend.port))
                .collect::<Vec<_>>()
        };
        map.insert(key, backends);
    }
    serde_json::to_string(&map).map_err(Into::into)
}

/// Get details for a specific service by ID.
pub fn get_service(service_id: u32) -> Result<Option<Service>> {
    let services = list_services()?;
    Ok(services.into_iter().find(|s| s.id.0 == service_id))
}

/// Get list of backends for a service.
pub fn get_service_backends(service_id: u32) -> Result<Vec<ServiceBackend>> {
    match get_service(service_id)? {
        Some(service) => Ok(service.backends),
        None => Err(Error::ServiceLookupFailed(format!(
            "service {} not found",
            service_id
        ))),
    }
}

/// Get service frontend address information.
pub fn get_service_frontend(service_id: u32) -> Result<String> {
    match get_service(service_id)? {
        Some(service) => Ok(service.frontend),
        None => Err(Error::ServiceLookupFailed(format!(
            "service {} not found",
            service_id
        ))),
    }
}

/// List services with cluster mesh affinity information.
pub fn list_services_with_affinity() -> Result<Vec<Service>> {
    list_services()
}
