//! Endpoint lifecycle management — ported from cilium/pkg/endpoint
//!
//! Implements the full endpoint lifecycle from creation through regeneration to disconnection.

pub mod lifecycle;
pub mod manager;

pub use lifecycle::{
    EndpointLifecycle, EndpointMetadata, EndpointState, RegenerationMetadata,
    RegenerationReason,
};
pub use manager::{EndpointManager, ManagedEndpoint, ManagerError, ManagerResult, ManagerStats};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Compact endpoint model (scaffold compatibility).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointModel {
    pub name: String,
    pub state: String,
    pub labels: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_model_serialization() {
        let model = EndpointModel {
            name: "test".to_string(),
            state: "ready".to_string(),
            labels: BTreeMap::new(),
        };
        let json = serde_json::to_string(&model).unwrap();
        let deserialized: EndpointModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, deserialized);
    }
}
