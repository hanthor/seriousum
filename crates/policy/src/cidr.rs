//! CIDR policy helpers.

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{PolicyError, Result};

/// A single CIDR rule with optional exception prefixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CIDRRule {
    /// The allowed prefix.
    pub cidr: IpNet,
    /// Prefixes excluded from the parent CIDR.
    pub except_cidrs: Vec<IpNet>,
}

impl CIDRRule {
    /// Creates a CIDR rule after validating exception prefixes.
    pub fn new(cidr: IpNet, except_cidrs: Vec<IpNet>) -> Result<Self> {
        for except in &except_cidrs {
            if cidr.addr().is_ipv4() != except.addr().is_ipv4() {
                return Err(PolicyError::InvalidCidr(format!(
                    "CIDR family mismatch: {cidr} vs {except}"
                )));
            }
            if !cidr.contains(&except.network()) || except.prefix_len() < cidr.prefix_len() {
                return Err(PolicyError::InvalidCidr(format!(
                    "exception prefix {except} must be contained within {cidr}"
                )));
            }
        }

        Ok(Self { cidr, except_cidrs })
    }

    /// Returns all prefixes referenced by the rule.
    #[must_use]
    pub fn prefixes(&self) -> Vec<IpNet> {
        let mut prefixes = vec![self.cidr];
        prefixes.extend(self.except_cidrs.iter().copied());
        prefixes
    }
}

/// Distilled CIDR policy for both directions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CIDRPolicy {
    /// Ingress CIDR rules.
    pub ingress: Vec<CIDRRule>,
    /// Egress CIDR rules.
    pub egress: Vec<CIDRRule>,
}

impl CIDRPolicy {
    /// Creates an empty CIDR policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an ingress CIDR rule.
    pub fn add_ingress_rule(&mut self, rule: CIDRRule) {
        self.ingress.push(rule);
    }

    /// Adds an egress CIDR rule.
    pub fn add_egress_rule(&mut self, rule: CIDRRule) {
        self.egress.push(rule);
    }

    /// Returns all unique prefixes referenced by this policy.
    #[must_use]
    pub fn generate_cidr_prefixes(&self) -> Vec<IpNet> {
        let mut rules = self.ingress.clone();
        rules.extend(self.egress.clone());
        generate_cidr_prefixes(&rules)
    }

    /// Returns true when the policy references every prefix from the provided rules.
    #[must_use]
    pub fn contains_all_rules(&self, rules: &[CIDRRule]) -> bool {
        let existing: std::collections::BTreeSet<String> = self
            .generate_cidr_prefixes()
            .into_iter()
            .map(|prefix| prefix.to_string())
            .collect();

        rules
            .iter()
            .flat_map(CIDRRule::prefixes)
            .all(|prefix| existing.contains(&prefix.to_string()))
    }
}

/// Generates a stable list of unique prefixes from a set of CIDR rules.
#[must_use]
pub fn generate_cidr_prefixes(rules: &[CIDRRule]) -> Vec<IpNet> {
    let mut unique = std::collections::BTreeMap::new();
    for prefix in rules.iter().flat_map(CIDRRule::prefixes) {
        unique.entry(prefix.to_string()).or_insert(prefix);
    }
    unique.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn generate_prefixes_includes_exceptions() {
        let rule = CIDRRule::new(
            IpNet::from_str("10.0.0.0/24").expect("valid parent CIDR"),
            vec![IpNet::from_str("10.0.0.128/25").expect("valid exception CIDR")],
        )
        .expect("valid rule");

        let prefixes = generate_cidr_prefixes(&[rule]);
        let rendered: Vec<String> = prefixes
            .into_iter()
            .map(|prefix| prefix.to_string())
            .collect();
        assert_eq!(rendered, vec!["10.0.0.0/24", "10.0.0.128/25"]);
    }

    #[test]
    fn contains_all_rules_checks_all_prefixes() {
        let rule = CIDRRule::new(
            IpNet::from_str("10.0.0.0/24").expect("valid parent CIDR"),
            vec![IpNet::from_str("10.0.0.128/25").expect("valid exception CIDR")],
        )
        .expect("valid rule");
        let mut policy = CIDRPolicy::new();
        policy.add_egress_rule(rule.clone());

        assert!(policy.contains_all_rules(&[rule]));
    }
}
