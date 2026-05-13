//! Rule types and rule evaluation.

use crate::{
    CIDRRule, CachedSelector, EndpointSelector, L4Policy, LabeledIdentity, Labels, Selector,
    TrafficDirection,
};

/// Per-direction default-deny configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DefaultDenyConfig {
    /// Optional ingress default-deny setting.
    pub ingress: Option<bool>,
    /// Optional egress default-deny setting.
    pub egress: Option<bool>,
}

impl DefaultDenyConfig {
    /// Creates a new default-deny configuration.
    #[must_use]
    pub fn new(ingress: Option<bool>, egress: Option<bool>) -> Self {
        Self { ingress, egress }
    }

    /// Resolves the effective ingress default-deny mode.
    #[must_use]
    pub fn ingress_enabled(&self, has_rules: bool) -> bool {
        self.ingress.unwrap_or(has_rules)
    }

    /// Resolves the effective egress default-deny mode.
    #[must_use]
    pub fn egress_enabled(&self, has_rules: bool) -> bool {
        self.egress.unwrap_or(has_rules)
    }
}

/// A distilled ingress rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IngressRule {
    /// Remote endpoint selectors allowed or denied by the rule.
    pub from_endpoints: Vec<CachedSelector>,
    /// Remote CIDR prefixes referenced by the rule.
    pub cidr_rules: Vec<CIDRRule>,
    /// Allowed L4 traffic.
    pub l4_policy: L4Policy,
    /// Whether this rule is a deny rule.
    pub deny: bool,
}

impl IngressRule {
    /// Creates an empty ingress rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a remote selector to the rule.
    #[must_use]
    pub fn with_selector(mut self, selector: impl Into<CachedSelector>) -> Self {
        self.from_endpoints.push(selector.into());
        self
    }

    /// Adds a CIDR rule to the ingress rule.
    #[must_use]
    pub fn with_cidr_rule(mut self, cidr_rule: CIDRRule) -> Self {
        self.cidr_rules.push(cidr_rule);
        self
    }

    /// Sets the L4 policy for the ingress rule.
    #[must_use]
    pub fn with_l4_policy(mut self, l4_policy: L4Policy) -> Self {
        self.l4_policy = l4_policy;
        self
    }

    /// Marks the rule as a deny rule.
    #[must_use]
    pub fn deny(mut self) -> Self {
        self.deny = true;
        self
    }

    /// Returns peer identities selected by this rule.
    #[must_use]
    pub fn selected_peers(&self, peers: &[LabeledIdentity]) -> Vec<LabeledIdentity> {
        if self.from_endpoints.is_empty() {
            return peers.to_vec();
        }

        peers
            .iter()
            .filter(|peer| {
                self.from_endpoints
                    .iter()
                    .any(|selector| selector.selects(peer))
            })
            .cloned()
            .collect()
    }
}

/// A distilled egress rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EgressRule {
    /// Remote endpoint selectors allowed or denied by the rule.
    pub to_endpoints: Vec<CachedSelector>,
    /// Remote CIDR prefixes referenced by the rule.
    pub cidr_rules: Vec<CIDRRule>,
    /// Allowed L4 traffic.
    pub l4_policy: L4Policy,
    /// Whether this rule is a deny rule.
    pub deny: bool,
}

impl EgressRule {
    /// Creates an empty egress rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a remote selector to the rule.
    #[must_use]
    pub fn with_selector(mut self, selector: impl Into<CachedSelector>) -> Self {
        self.to_endpoints.push(selector.into());
        self
    }

    /// Adds a CIDR rule to the egress rule.
    #[must_use]
    pub fn with_cidr_rule(mut self, cidr_rule: CIDRRule) -> Self {
        self.cidr_rules.push(cidr_rule);
        self
    }

    /// Sets the L4 policy for the egress rule.
    #[must_use]
    pub fn with_l4_policy(mut self, l4_policy: L4Policy) -> Self {
        self.l4_policy = l4_policy;
        self
    }

    /// Marks the rule as a deny rule.
    #[must_use]
    pub fn deny(mut self) -> Self {
        self.deny = true;
        self
    }

    /// Returns peer identities selected by this rule.
    #[must_use]
    pub fn selected_peers(&self, peers: &[LabeledIdentity]) -> Vec<LabeledIdentity> {
        if self.to_endpoints.is_empty() {
            return peers.to_vec();
        }

        peers
            .iter()
            .filter(|peer| {
                self.to_endpoints
                    .iter()
                    .any(|selector| selector.selects(peer))
            })
            .cloned()
            .collect()
    }
}

/// Core policy rule ported from `pkg/policy/rule.go`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Exact-match labels used to identify the subject endpoints.
    pub labels: Labels,
    /// Full cached selector used for subject matching.
    pub selector: CachedSelector,
    /// Ingress policy sections.
    pub ingress_rules: Vec<IngressRule>,
    /// Egress policy sections.
    pub egress_rules: Vec<EgressRule>,
    /// Per-direction default-deny configuration.
    pub enable_default_deny: DefaultDenyConfig,
}

impl Rule {
    /// Creates a rule targeting the provided endpoint labels.
    #[must_use]
    pub fn new(labels: Labels) -> Self {
        let selector = Selector::new(labels.clone());
        Self {
            labels,
            selector: CachedSelector::new(selector),
            ingress_rules: Vec::new(),
            egress_rules: Vec::new(),
            enable_default_deny: DefaultDenyConfig::default(),
        }
    }

    /// Replaces the subject selector while keeping `labels` in sync.
    #[must_use]
    pub fn with_selector(mut self, selector: Selector) -> Self {
        self.labels.clone_from(&selector.match_labels);
        self.selector = CachedSelector::new(selector);
        self
    }

    /// Adds an ingress section.
    #[must_use]
    pub fn with_ingress_rule(mut self, rule: IngressRule) -> Self {
        self.ingress_rules.push(rule);
        self
    }

    /// Adds an egress section.
    #[must_use]
    pub fn with_egress_rule(mut self, rule: EgressRule) -> Self {
        self.egress_rules.push(rule);
        self
    }

    /// Sets the default-deny configuration.
    #[must_use]
    pub fn with_enable_default_deny(mut self, config: DefaultDenyConfig) -> Self {
        self.enable_default_deny = config;
        self
    }

    /// Returns true when this rule selects the provided endpoint labels.
    #[must_use]
    pub fn matches_endpoint(&self, endpoint_labels: &Labels) -> bool {
        self.selector.matches(endpoint_labels)
    }

    /// Returns true when the rule contributes ingress policy for the endpoint.
    #[must_use]
    pub fn resolves_ingress_for_endpoint(&self, endpoint_labels: &Labels) -> bool {
        self.matches_endpoint(endpoint_labels) && !self.ingress_rules.is_empty()
    }

    /// Returns true when the rule contributes egress policy for the endpoint.
    #[must_use]
    pub fn resolves_egress_for_endpoint(&self, endpoint_labels: &Labels) -> bool {
        self.matches_endpoint(endpoint_labels) && !self.egress_rules.is_empty()
    }

    /// Returns the effective ingress default-deny mode for this rule.
    #[must_use]
    pub fn default_deny_ingress(&self) -> bool {
        self.enable_default_deny
            .ingress_enabled(!self.ingress_rules.is_empty())
    }

    /// Returns the effective egress default-deny mode for this rule.
    #[must_use]
    pub fn default_deny_egress(&self) -> bool {
        self.enable_default_deny
            .egress_enabled(!self.egress_rules.is_empty())
    }
}

/// Compatibility wrapper preserved for existing benchmarks and call sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// Traffic direction of the rule.
    pub direction: TrafficDirection,
    /// Subject endpoint selector.
    pub subject_selector: EndpointSelector,
    /// Peer endpoint selector.
    pub peer_selector: EndpointSelector,
    /// L4 policy applied to the matched traffic.
    pub l4_policy: L4Policy,
}

impl PolicyRule {
    /// Creates a compatibility rule for a single traffic direction.
    #[must_use]
    pub fn new(direction: TrafficDirection) -> Self {
        Self {
            direction,
            subject_selector: EndpointSelector::empty(),
            peer_selector: EndpointSelector::empty(),
            l4_policy: L4Policy::new(),
        }
    }

    /// Sets the subject selector.
    #[must_use]
    pub fn with_subject_selector(mut self, selector: EndpointSelector) -> Self {
        self.subject_selector = selector;
        self
    }

    /// Sets the peer selector.
    #[must_use]
    pub fn with_peer_selector(mut self, selector: EndpointSelector) -> Self {
        self.peer_selector = selector;
        self
    }

    /// Sets the L4 policy.
    #[must_use]
    pub fn with_l4_policy(mut self, policy: L4Policy) -> Self {
        self.l4_policy = policy;
        self
    }

    /// Returns true when the peer selector matches the provided labels.
    #[must_use]
    pub fn applies_to(&self, peer_labels: &Labels) -> bool {
        self.peer_selector.matches(peer_labels)
    }
}

impl From<PolicyRule> for Rule {
    fn from(rule: PolicyRule) -> Self {
        let subject = rule.subject_selector.clone();
        let peer = CachedSelector::new(rule.peer_selector.clone());
        let mut core = Rule::new(subject.match_labels.clone()).with_selector(subject);

        match rule.direction {
            TrafficDirection::Ingress => {
                core.ingress_rules.push(
                    IngressRule::new()
                        .with_selector(peer)
                        .with_l4_policy(rule.l4_policy),
                );
                core.enable_default_deny = DefaultDenyConfig::new(Some(true), None);
            }
            TrafficDirection::Egress => {
                core.egress_rules.push(
                    EgressRule::new()
                        .with_selector(peer)
                        .with_l4_policy(rule.l4_policy),
                );
                core.enable_default_deny = DefaultDenyConfig::new(None, Some(true));
            }
        }

        core
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RequirementOperator;
    use std::collections::HashMap;

    #[test]
    fn rule_matches_endpoint_labels() {
        let rule = Rule::new(HashMap::from([("app".to_string(), "backend".to_string())]));
        let endpoint = HashMap::from([("app".to_string(), "backend".to_string())]);

        assert!(rule.matches_endpoint(&endpoint));
    }

    #[test]
    fn rule_respects_selector_requirements() {
        let rule = Rule::new(HashMap::new()).with_selector(Selector::empty().with_requirement(
            crate::Requirement::new("env", RequirementOperator::In, ["prod"]),
        ));
        let endpoint = HashMap::from([("env".to_string(), "prod".to_string())]);

        assert!(rule.matches_endpoint(&endpoint));
    }

    #[test]
    fn resolves_direction_only_for_matching_endpoints() {
        let endpoint = HashMap::from([("app".to_string(), "backend".to_string())]);
        let rule = Rule::new(HashMap::from([("app".to_string(), "backend".to_string())]))
            .with_ingress_rule(IngressRule::new())
            .with_egress_rule(EgressRule::new());

        assert!(rule.resolves_ingress_for_endpoint(&endpoint));
        assert!(rule.resolves_egress_for_endpoint(&endpoint));
    }

    #[test]
    fn policy_rule_applies_to_peer_labels() {
        let policy_rule = PolicyRule::new(TrafficDirection::Ingress)
            .with_peer_selector(Selector::empty().with_label("app", "client"));
        let peer = HashMap::from([("app".to_string(), "client".to_string())]);

        assert!(policy_rule.applies_to(&peer));
    }
}
