//! FQDN-based policy enforcement
//!
//! Maps FQDNs to security identities and policy rules.

use crate::types::{FqdnSelector, IpCidr};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Policy selector for ingress/egress rules
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicySelector {
    /// FQDN pattern to match
    pub fqdn: String,

    /// Optional source/destination identity label
    pub identity_label: Option<String>,

    /// Protocol (TCP=6, UDP=17, etc.)
    pub protocol: Option<u8>,

    /// Port number (0 = any)
    pub port: Option<u16>,
}

impl PolicySelector {
    /// Creates a new policy selector with FQDN
    pub fn new(fqdn: impl Into<String>) -> Self {
        Self {
            fqdn: fqdn.into(),
            identity_label: None,
            protocol: None,
            port: None,
        }
    }

    /// Sets identity label for selector
    pub fn with_identity(mut self, label: impl Into<String>) -> Self {
        self.identity_label = Some(label.into());
        self
    }

    /// Sets protocol and port for selector
    pub fn with_protocol_port(mut self, protocol: u8, port: u16) -> Self {
        self.protocol = Some(protocol);
        self.port = Some(port);
        self
    }
}

/// FQDN policy rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FqdnPolicy {
    /// Policy name
    pub name: String,

    /// FQDN selectors to allow
    pub allow_fqdns: Vec<FqdnSelector>,

    /// Associated CIDRs (resolved from FQDNs)
    pub associated_cidrs: Vec<IpCidr>,

    /// Policy selectors for enforcement
    pub selectors: Vec<PolicySelector>,
}

impl FqdnPolicy {
    /// Creates a new FQDN policy
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            allow_fqdns: Vec::new(),
            associated_cidrs: Vec::new(),
            selectors: Vec::new(),
        }
    }

    /// Adds an allowed FQDN selector
    pub fn with_allow_fqdn(mut self, selector: FqdnSelector) -> Self {
        self.allow_fqdns.push(selector);
        self
    }

    /// Adds an associated CIDR
    pub fn with_cidr(mut self, cidr: IpCidr) -> Self {
        self.associated_cidrs.push(cidr);
        self
    }

    /// Adds a policy selector
    pub fn with_selector(mut self, selector: PolicySelector) -> Self {
        self.selectors.push(selector);
        self
    }
}

/// FQDN policy repository
#[derive(Debug, Clone)]
pub struct FqdnPolicyRepository {
    /// Policies by name
    policies: Arc<DashMap<String, FqdnPolicy>>,

    /// FQDN to policies mapping
    fqdn_to_policies: Arc<DashMap<String, Vec<String>>>,
}

impl FqdnPolicyRepository {
    /// Creates a new policy repository
    pub fn new() -> Self {
        Self {
            policies: Arc::new(DashMap::new()),
            fqdn_to_policies: Arc::new(DashMap::new()),
        }
    }

    /// Adds a policy to the repository
    pub fn add_policy(&self, policy: FqdnPolicy) {
        let name = policy.name.clone();

        // Index by FQDN
        for fqdn_sel in &policy.allow_fqdns {
            self.fqdn_to_policies
                .entry(fqdn_sel.pattern.clone())
                .or_default()
                .push(name.clone());
        }

        self.policies.insert(name, policy);
    }

    /// Gets a policy by name
    pub fn get_policy(&self, name: &str) -> Option<FqdnPolicy> {
        self.policies.get(name).map(|p| p.clone())
    }

    /// Finds all policies for a given FQDN
    pub fn find_policies_for_fqdn(&self, fqdn: &str) -> Vec<FqdnPolicy> {
        let mut result = Vec::new();

        if let Some(policy_names) = self.fqdn_to_policies.get(fqdn) {
            for name in policy_names.value() {
                if let Some(policy) = self.policies.get(name) {
                    result.push(policy.clone());
                }
            }
        }

        result
    }

    /// Removes a policy by name
    pub fn remove_policy(&self, name: &str) -> Option<FqdnPolicy> {
        if let Some((_, policy)) = self.policies.remove(name) {
            // Remove from FQDN index
            for fqdn_sel in &policy.allow_fqdns {
                if let Some(mut policies) = self.fqdn_to_policies.get_mut(&fqdn_sel.pattern) {
                    policies.retain(|p| p != name);
                }
            }
            Some(policy)
        } else {
            None
        }
    }

    /// Returns the number of policies
    pub fn len(&self) -> usize {
        self.policies.len()
    }

    /// Checks if repository is empty
    pub fn is_empty(&self) -> bool {
        self.policies.is_empty()
    }

    /// Clears all policies
    pub fn clear(&self) {
        self.policies.clear();
        self.fqdn_to_policies.clear();
    }
}

impl Default for FqdnPolicyRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_selector_creation() {
        let selector = PolicySelector::new("example.com");
        assert_eq!(selector.fqdn, "example.com");
    }

    #[test]
    fn fqdn_policy_creation() {
        let policy = FqdnPolicy::new("allow-example");
        assert_eq!(policy.name, "allow-example");
        assert!(policy.allow_fqdns.is_empty());
    }

    #[test]
    fn policy_repository_add_and_get() {
        let repo = FqdnPolicyRepository::new();
        let policy = FqdnPolicy::new("test-policy");

        repo.add_policy(policy.clone());
        let retrieved = repo.get_policy("test-policy");

        assert_eq!(retrieved, Some(policy));
    }

    #[test]
    fn policy_repository_find_by_fqdn() {
        let repo = FqdnPolicyRepository::new();
        let fqdn_sel = FqdnSelector::new("example.com");
        let policy = FqdnPolicy::new("test-policy").with_allow_fqdn(fqdn_sel);

        repo.add_policy(policy);
        let found = repo.find_policies_for_fqdn("example.com");

        assert_eq!(found.len(), 1);
    }

    #[test]
    fn policy_repository_remove() {
        let repo = FqdnPolicyRepository::new();
        let policy = FqdnPolicy::new("test-policy");

        repo.add_policy(policy);
        assert_eq!(repo.len(), 1);

        let removed = repo.remove_policy("test-policy");
        assert!(removed.is_some());
        assert!(repo.is_empty());
    }

    #[test]
    fn policy_repository_clear() {
        let repo = FqdnPolicyRepository::new();

        repo.add_policy(FqdnPolicy::new("policy1"));
        repo.add_policy(FqdnPolicy::new("policy2"));
        assert_eq!(repo.len(), 2);

        repo.clear();
        assert!(repo.is_empty());
    }
}
