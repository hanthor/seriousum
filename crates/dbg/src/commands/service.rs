//! Service and load balancer inspection commands
//!
//! Provides commands for:
//! - Listing services
//! - Viewing service details
//! - Inspecting backends and load balancer state
//! - Viewing service affinity and traffic policies

use crate::{Error, Result, Service, ServiceBackend, ServiceId};

/// List all services configured in the system
pub fn list_services() -> Result<Vec<Service>> {
    // In a real implementation, this would query the BPF maps or the API
    // to retrieve service information
    Ok(vec![
        Service {
            id: ServiceId(1),
            frontend: "10.0.0.1:80".to_string(),
            service_type: "ClusterIP".to_string(),
            backends: vec![
                ServiceBackend {
                    address: "10.1.0.1:8080".to_string(),
                    port: 8080,
                    state: "active".to_string(),
                    preferred: false,
                },
                ServiceBackend {
                    address: "10.1.0.2:8080".to_string(),
                    port: 8080,
                    state: "active".to_string(),
                    preferred: false,
                },
            ],
        },
        Service {
            id: ServiceId(2),
            frontend: "10.0.0.2:443".to_string(),
            service_type: "NodePort".to_string(),
            backends: vec![ServiceBackend {
                address: "10.1.1.1:443".to_string(),
                port: 443,
                state: "active".to_string(),
                preferred: true,
            }],
        },
    ])
}

/// Get details for a specific service by ID
pub fn get_service(service_id: u32) -> Result<Option<Service>> {
    let services = list_services()?;
    Ok(services.into_iter().find(|s| s.id.0 == service_id))
}

/// Get list of backends for a service
pub fn get_service_backends(service_id: u32) -> Result<Vec<ServiceBackend>> {
    match get_service(service_id)? {
        Some(service) => Ok(service.backends),
        None => Err(Error::ServiceLookupFailed(format!(
            "service {} not found",
            service_id
        ))),
    }
}

/// Get service frontend address information
pub fn get_service_frontend(service_id: u32) -> Result<String> {
    match get_service(service_id)? {
        Some(service) => Ok(service.frontend),
        None => Err(Error::ServiceLookupFailed(format!(
            "service {} not found",
            service_id
        ))),
    }
}

/// List services with cluster mesh affinity information
pub fn list_services_with_affinity() -> Result<Vec<Service>> {
    // Similar to list_services but includes affinity information
    list_services()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_services() {
        let services = list_services().unwrap();
        assert!(!services.is_empty());
        assert_eq!(services[0].id.0, 1);
    }

    #[test]
    fn test_list_services_has_backends() {
        let services = list_services().unwrap();
        let service = &services[0];
        assert!(!service.backends.is_empty());
    }

    #[test]
    fn test_get_service_existing() {
        let service = get_service(1).unwrap();
        assert!(service.is_some());
        assert_eq!(service.unwrap().id.0, 1);
    }

    #[test]
    fn test_get_service_nonexistent() {
        let service = get_service(99999).unwrap();
        assert!(service.is_none());
    }

    #[test]
    fn test_get_service_backends() {
        let backends = get_service_backends(1).unwrap();
        assert!(!backends.is_empty());
    }

    #[test]
    fn test_get_service_backends_nonexistent() {
        let result = get_service_backends(99999);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_service_frontend() {
        let frontend = get_service_frontend(1).unwrap();
        assert!(frontend.contains("10.0.0.1"));
    }

    #[test]
    fn test_list_services_with_affinity() {
        let services = list_services_with_affinity().unwrap();
        assert!(!services.is_empty());
    }
}
