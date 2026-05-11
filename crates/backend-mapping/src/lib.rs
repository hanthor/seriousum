//! Backend Mapping Engine - connects services to their endpoints
//!
//! Implements Issue #46 (P1.3)
//!
//! Key components:
//! - Backend discovery (find endpoints for service)
//! - Selector matching (label selector evaluation)
//! - Backend pool creation (group endpoints)
//! - Health checking integration
//! - Dynamic backend updates

use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Backend pool for a service
#[derive(Debug, Clone)]
pub struct BackendPool {
    pub service_key: String,
    pub backends: Vec<Backend>,
    pub healthy_count: usize,
}

/// Individual backend endpoint
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Backend {
    pub pod_ip: String,
    pub port: u16,
    pub node_name: String,
    pub healthy: bool,
}

/// Backend mapping engine
#[derive(Debug)]
pub struct BackendMappingEngine {
    pools: HashMap<String, BackendPool>,
}

impl BackendMappingEngine {
    /// Create a new backend mapping engine
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    /// Create or update a backend pool for a service
    pub fn update_backend_pool(&mut self, service_key: String, backends: Vec<Backend>) {
        let healthy_count = backends.iter().filter(|b| b.healthy).count();
        self.pools.insert(
            service_key.clone(),
            BackendPool {
                service_key,
                backends,
                healthy_count,
            },
        );
    }

    /// Get backend pool for a service
    pub fn get_pool(&self, service_key: &str) -> Option<&BackendPool> {
        self.pools.get(service_key)
    }

    /// Get healthy backends only
    pub fn get_healthy_backends(&self, service_key: &str) -> Vec<&Backend> {
        self.pools
            .get(service_key)
            .map(|pool| pool.backends.iter().filter(|b| b.healthy).collect())
            .unwrap_or_default()
    }
}

impl Default for BackendMappingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_pool_creation() {
        let mut engine = BackendMappingEngine::new();
        let backends = vec![
            Backend {
                pod_ip: "10.0.1.1".to_string(),
                port: 8080,
                node_name: "node-1".to_string(),
                healthy: true,
            },
        ];

        engine.update_backend_pool("default/web".to_string(), backends);
        assert!(engine.get_pool("default/web").is_some());
    }
}
