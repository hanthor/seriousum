//! Policy repository and policy distillation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use tracing::debug;

use crate::{
    CIDRPolicy, EgressRule, EndpointIdentity, Key, L4Traffic, LabeledIdentity, Labels,
    MapStateEntry, MapStateMap, PolicyError, PolicyRule, Protocol, Result, Rule, TrafficDirection,
    insert_map_state,
};

/// Distilled L4 policy map: each L4 tuple maps to the selected peer identities.
pub type L4PolicyMap = BTreeMap<L4Traffic, BTreeSet<u32>>;

#[derive(Debug, Default)]
struct RepositoryState {
    revision: u64,
    rules: Vec<Rule>,
}

/// In-memory policy repository.
#[derive(Debug, Clone, Default)]
pub struct Repository {
    state: Arc<RwLock<RepositoryState>>,
}

/// Compatibility alias preserved for existing benchmarks and callers.
pub type PolicyRepository = Repository;

/// Distilled selector policy for one endpoint identity.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectorPolicy {
    /// Repository revision used to compute the policy.
    pub revision: u64,
    /// Distilled ingress peer identities per L4 tuple.
    pub ingress_policy: L4PolicyMap,
    /// Distilled egress peer identities per L4 tuple.
    pub egress_policy: L4PolicyMap,
    /// Distilled CIDR policy.
    pub cidr_policy: CIDRPolicy,
    /// Whether ingress is in default-deny mode.
    pub deny_ingress: bool,
    /// Whether egress is in default-deny mode.
    pub deny_egress: bool,
    /// Final distilled datapath map state.
    pub map_state: MapStateMap,
}

impl SelectorPolicy {
    /// Returns true when ingress allows the given identity, protocol, and port.
    #[must_use]
    pub fn ingress_allows(&self, identity: u32, protocol: Protocol, port: u16) -> bool {
        Self::policy_allows(&self.ingress_policy, identity, protocol, port)
    }

    /// Returns true when egress allows the given identity, protocol, and port.
    #[must_use]
    pub fn egress_allows(&self, identity: u32, protocol: Protocol, port: u16) -> bool {
        Self::policy_allows(&self.egress_policy, identity, protocol, port)
    }

    fn policy_allows(
        policy_map: &L4PolicyMap,
        identity: u32,
        protocol: Protocol,
        port: u16,
    ) -> bool {
        policy_map
            .iter()
            .any(|(traffic, peers)| traffic.matches(protocol, port) && peers.contains(&identity))
    }
}

impl Repository {
    /// Creates an empty repository.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(RepositoryState {
                revision: 1,
                rules: Vec::new(),
            })),
        }
    }

    /// Returns the current repository revision.
    pub fn revision(&self) -> Result<u64> {
        Ok(self
            .state
            .read()
            .map_err(|_| PolicyError::ConcurrentModification)?
            .revision)
    }

    /// Returns a snapshot of all rules.
    pub fn rules(&self) -> Result<Vec<Rule>> {
        Ok(self
            .state
            .read()
            .map_err(|_| PolicyError::ConcurrentModification)?
            .rules
            .clone())
    }

    /// Adds a core rule to the repository and returns the new revision.
    pub fn add_rule(&self, rule: Rule) -> Result<u64> {
        let mut state = self
            .state
            .write()
            .map_err(|_| PolicyError::ConcurrentModification)?;
        state.rules.push(rule);
        state.revision += 1;
        Ok(state.revision)
    }

    /// Deletes rules whose subject labels match exactly and returns the count.
    pub fn delete_rule_by_labels(&self, labels: &Labels) -> Result<usize> {
        let mut state = self
            .state
            .write()
            .map_err(|_| PolicyError::ConcurrentModification)?;
        let before = state.rules.len();
        state.rules.retain(|rule| &rule.labels != labels);
        let deleted = before.saturating_sub(state.rules.len());
        if deleted > 0 {
            state.revision += 1;
        }
        Ok(deleted)
    }

    /// Adds a compatibility ingress rule.
    pub fn add_ingress_rule(&self, _id: impl Into<String>, rule: PolicyRule) -> Result<u64> {
        if rule.direction != TrafficDirection::Ingress {
            return Err(PolicyError::InvalidRule(
                "cannot add non-ingress rule to ingress set".to_string(),
            ));
        }
        self.add_rule(rule.into())
    }

    /// Adds a compatibility egress rule.
    pub fn add_egress_rule(&self, _id: impl Into<String>, rule: PolicyRule) -> Result<u64> {
        if rule.direction != TrafficDirection::Egress {
            return Err(PolicyError::InvalidRule(
                "cannot add non-egress rule to egress set".to_string(),
            ));
        }
        self.add_rule(rule.into())
    }

    /// Resolves policy for one endpoint against a set of peer identities.
    pub fn resolve_policy(
        &self,
        endpoint: &LabeledIdentity,
        peers: &[LabeledIdentity],
    ) -> Result<SelectorPolicy> {
        let state = self
            .state
            .read()
            .map_err(|_| PolicyError::ConcurrentModification)?;

        let matching_rules = state
            .rules
            .iter()
            .filter(|rule| rule.matches_endpoint(&endpoint.labels))
            .cloned()
            .collect::<Vec<_>>();

        let mut policy = SelectorPolicy {
            revision: state.revision,
            ..SelectorPolicy::default()
        };

        let ingress_rules_present = matching_rules
            .iter()
            .any(|rule| !rule.ingress_rules.is_empty());
        let egress_rules_present = matching_rules
            .iter()
            .any(|rule| !rule.egress_rules.is_empty());
        policy.deny_ingress = matching_rules.iter().any(Rule::default_deny_ingress);
        policy.deny_egress = matching_rules.iter().any(Rule::default_deny_egress);

        if ingress_rules_present && !policy.deny_ingress {
            debug!(
                identity = endpoint.identity.id,
                "ingress policy selected without default deny"
            );
        }
        if egress_rules_present && !policy.deny_egress {
            debug!(
                identity = endpoint.identity.id,
                "egress policy selected without default deny"
            );
        }

        for rule in matching_rules {
            for ingress in &rule.ingress_rules {
                Self::apply_ingress_rule(&mut policy, ingress, peers);
            }
            for egress in &rule.egress_rules {
                Self::apply_egress_rule(&mut policy, egress, peers);
            }
        }

        Ok(policy)
    }

    /// Compatibility entry point preserved for existing benchmarks.
    pub fn distill_policy(
        &self,
        endpoint_identity: EndpointIdentity,
        endpoint_labels: &Labels,
    ) -> Result<SelectorPolicy> {
        let endpoint = LabeledIdentity::new(endpoint_identity.id, endpoint_labels.clone());
        self.resolve_policy(&endpoint, &[])
    }

    fn apply_ingress_rule(
        policy: &mut SelectorPolicy,
        ingress: &crate::IngressRule,
        peers: &[LabeledIdentity],
    ) {
        let selected_peers = ingress.selected_peers(peers);
        for cidr_rule in &ingress.cidr_rules {
            policy.cidr_policy.add_ingress_rule(cidr_rule.clone());
        }
        for traffic in ingress.l4_policy.entries() {
            let peer_entry = policy.ingress_policy.entry(traffic.clone()).or_default();
            for peer in &selected_peers {
                peer_entry.insert(peer.identity.id);
                Self::insert_entries(
                    &mut policy.map_state,
                    peer.identity.id,
                    TrafficDirection::Ingress,
                    &traffic,
                    ingress.deny,
                    ingress.l4_policy.proxy_required,
                    ingress.l4_policy.authentication_required,
                );
            }
        }
    }

    fn apply_egress_rule(
        policy: &mut SelectorPolicy,
        egress: &EgressRule,
        peers: &[LabeledIdentity],
    ) {
        let selected_peers = egress.selected_peers(peers);
        for cidr_rule in &egress.cidr_rules {
            policy.cidr_policy.add_egress_rule(cidr_rule.clone());
        }
        for traffic in egress.l4_policy.entries() {
            let peer_entry = policy.egress_policy.entry(traffic.clone()).or_default();
            for peer in &selected_peers {
                peer_entry.insert(peer.identity.id);
                Self::insert_entries(
                    &mut policy.map_state,
                    peer.identity.id,
                    TrafficDirection::Egress,
                    &traffic,
                    egress.deny,
                    egress.l4_policy.proxy_required,
                    egress.l4_policy.authentication_required,
                );
            }
        }
    }

    fn insert_entries(
        map_state: &mut MapStateMap,
        identity: u32,
        direction: TrafficDirection,
        traffic: &L4Traffic,
        deny: bool,
        proxy_required: bool,
        authentication_required: bool,
    ) {
        let ports: Vec<u16> = if traffic.is_wildcard() {
            vec![0]
        } else {
            (traffic.port_start..=traffic.port_end).collect()
        };
        let nexthdr = traffic.protocol.as_u8();
        let proxy_port = if proxy_required { 15000 } else { 0 };
        let entry = MapStateEntry::new(proxy_port, deny, authentication_required);

        for port in ports {
            insert_map_state(
                map_state,
                Key::new(identity, port, nexthdr, direction),
                entry.clone(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CIDRRule, DefaultDenyConfig, IngressRule, L4Policy, Selector};
    use ipnet::IpNet;
    use std::collections::HashMap;
    use std::str::FromStr;

    fn labeled_identity(id: u32, labels: &[(&str, &str)]) -> LabeledIdentity {
        LabeledIdentity::new(
            id,
            labels
                .iter()
                .map(|(key, value)| (String::from(*key), String::from(*value)))
                .collect::<HashMap<_, _>>(),
        )
    }

    #[test]
    fn repository_add_delete_and_resolve_policy() {
        let repo = Repository::new();
        let rule = Rule::new(HashMap::from([("app".to_string(), "backend".to_string())]))
            .with_ingress_rule(
                IngressRule::new().with_selector(Selector::empty().with_label("app", "frontend")),
            );

        let revision = repo.add_rule(rule).expect("rule added");
        assert_eq!(revision, 2);
        assert_eq!(repo.rules().expect("rules snapshot").len(), 1);

        let endpoint = labeled_identity(300, &[("app", "backend")]);
        let peer = labeled_identity(100, &[("app", "frontend")]);
        let policy = repo
            .resolve_policy(&endpoint, &[peer])
            .expect("policy resolved");
        assert!(policy.deny_ingress);

        let deleted = repo
            .delete_rule_by_labels(&HashMap::from([("app".to_string(), "backend".to_string())]))
            .expect("rule deleted");
        assert_eq!(deleted, 1);
    }

    #[test]
    fn distillation_produces_expected_allowed_sets() {
        let repo = Repository::new();
        let mut ingress_l4 = L4Policy::new();
        ingress_l4.add_allowed(L4Traffic::new(Protocol::TCP, 80));
        let mut egress_l4 = L4Policy::new();
        egress_l4.add_allowed(L4Traffic::new(Protocol::TCP, 5432));

        let cidr_rule = CIDRRule::new(
            IpNet::from_str("10.0.0.0/24").expect("valid CIDR"),
            vec![IpNet::from_str("10.0.0.128/25").expect("valid CIDR")],
        )
        .expect("valid CIDR rule");

        let rule = Rule::new(HashMap::from([("app".to_string(), "backend".to_string())]))
            .with_ingress_rule(
                IngressRule::new()
                    .with_selector(Selector::empty().with_label("app", "frontend"))
                    .with_l4_policy(ingress_l4),
            )
            .with_egress_rule(
                crate::EgressRule::new()
                    .with_selector(Selector::empty().with_label("app", "db"))
                    .with_cidr_rule(cidr_rule)
                    .with_l4_policy(egress_l4),
            )
            .with_enable_default_deny(DefaultDenyConfig::new(Some(true), Some(true)));

        repo.add_rule(rule).expect("rule added");

        let endpoint = labeled_identity(300, &[("app", "backend")]);
        let frontend = labeled_identity(100, &[("app", "frontend")]);
        let database = labeled_identity(200, &[("app", "db")]);
        let policy = repo
            .resolve_policy(&endpoint, &[frontend.clone(), database.clone()])
            .expect("policy resolved");

        assert!(policy.deny_ingress);
        assert!(policy.deny_egress);
        assert!(policy.ingress_allows(frontend.identity.id, Protocol::TCP, 80));
        assert!(!policy.ingress_allows(database.identity.id, Protocol::TCP, 80));
        assert!(policy.egress_allows(database.identity.id, Protocol::TCP, 5432));
        assert_eq!(policy.cidr_policy.generate_cidr_prefixes().len(), 2);
        assert_eq!(
            policy
                .map_state
                .get(&Key::new(100, 80, 6, TrafficDirection::Ingress)),
            Some(&MapStateEntry::allow(0, false))
        );
        assert_eq!(
            policy
                .map_state
                .get(&Key::new(200, 5432, 6, TrafficDirection::Egress)),
            Some(&MapStateEntry::allow(0, false))
        );
    }

    #[test]
    fn non_default_deny_rules_keep_default_allow_flags() {
        let repo = Repository::new();
        let rule = Rule::new(HashMap::from([("app".to_string(), "backend".to_string())]))
            .with_egress_rule(
                crate::EgressRule::new().with_selector(Selector::empty().with_label("app", "db")),
            )
            .with_enable_default_deny(DefaultDenyConfig::new(Some(false), Some(false)));
        repo.add_rule(rule).expect("rule added");

        let endpoint = labeled_identity(300, &[("app", "backend")]);
        let database = labeled_identity(200, &[("app", "db")]);
        let policy = repo
            .resolve_policy(&endpoint, &[database])
            .expect("policy resolved");

        assert!(!policy.deny_ingress);
        assert!(!policy.deny_egress);
    }
}
