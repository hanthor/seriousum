//! Selector types for endpoint and identity matching.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{LabeledIdentity, Labels};

/// Selector requirement operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequirementOperator {
    /// The label value must be within the provided set.
    In,
    /// The label value must not be within the provided set.
    NotIn,
    /// The label key must be present.
    Exists,
    /// The label key must be absent.
    DoesNotExist,
}

/// A single selector requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// Label key inspected by the requirement.
    pub key: String,
    /// Matching operator.
    pub operator: RequirementOperator,
    /// Allowed or disallowed values.
    pub values: BTreeSet<String>,
}

impl Requirement {
    /// Creates a new requirement.
    #[must_use]
    pub fn new<I, S>(key: impl Into<String>, operator: RequirementOperator, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            key: key.into(),
            operator,
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns true when the requirement matches the provided labels.
    #[must_use]
    pub fn matches(&self, labels: &Labels) -> bool {
        let value = labels.get(&self.key);
        match self.operator {
            RequirementOperator::In => value.is_some_and(|item| self.values.contains(item)),
            RequirementOperator::NotIn => value.is_none_or(|item| !self.values.contains(item)),
            RequirementOperator::Exists => value.is_some(),
            RequirementOperator::DoesNotExist => value.is_none(),
        }
    }
}

/// Endpoint selector built from match labels and requirements.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Selector {
    /// Labels that must match exactly.
    pub match_labels: Labels,
    /// Additional requirements evaluated with AND semantics.
    pub requirements: Vec<Requirement>,
}

impl Selector {
    /// Creates a selector from exact-match labels.
    #[must_use]
    pub fn new(match_labels: Labels) -> Self {
        Self {
            match_labels,
            requirements: Vec::new(),
        }
    }

    /// Creates an empty wildcard selector.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a selector that matches every endpoint.
    #[must_use]
    pub fn match_all() -> Self {
        Self::default()
    }

    /// Adds an exact-match label to the selector.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_labels.insert(key.into(), value.into());
        self
    }

    /// Adds a requirement to the selector.
    #[must_use]
    pub fn with_requirement(mut self, requirement: Requirement) -> Self {
        self.requirements.push(requirement);
        self
    }

    /// Returns true when the selector is a wildcard.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.match_labels.is_empty() && self.requirements.is_empty()
    }

    /// Returns a stable cache key for the selector.
    #[must_use]
    pub fn key(&self) -> String {
        let labels = self
            .match_labels
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",");

        let requirements = self
            .requirements
            .iter()
            .map(|requirement| {
                let values = requirement
                    .values
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("|");
                format!("{}:{:?}:{}", requirement.key, requirement.operator, values)
            })
            .collect::<Vec<_>>()
            .join(",");

        format!("labels[{labels}]-requirements[{requirements}]")
    }

    /// Returns true when the selector matches the provided labels.
    #[must_use]
    pub fn matches(&self, endpoint_labels: &Labels) -> bool {
        self.match_labels
            .iter()
            .all(|(key, value)| endpoint_labels.get(key) == Some(value))
            && self
                .requirements
                .iter()
                .all(|requirement| requirement.matches(endpoint_labels))
    }
}

/// Cached selector wrapper mirroring Cilium's selector cache surface.
#[derive(Debug, Clone)]
pub struct CachedSelector {
    selector: Arc<Selector>,
    key: String,
}

impl CachedSelector {
    /// Creates a cached selector from a selector definition.
    #[must_use]
    pub fn new(selector: Selector) -> Self {
        let key = selector.key();
        Self {
            selector: Arc::new(selector),
            key,
        }
    }

    /// Returns the stable selector key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns true when the cached selector is a wildcard.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.selector.is_wildcard()
    }

    /// Returns true when the selector matches the provided labels.
    #[must_use]
    pub fn matches(&self, labels: &Labels) -> bool {
        self.selector.matches(labels)
    }

    /// Returns true when the selector matches a labeled identity.
    #[must_use]
    pub fn selects(&self, identity: &LabeledIdentity) -> bool {
        self.matches(&identity.labels)
    }

    /// Returns the underlying selector.
    #[must_use]
    pub fn selector(&self) -> &Selector {
        self.selector.as_ref()
    }
}

impl PartialEq for CachedSelector {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for CachedSelector {}

impl From<Selector> for CachedSelector {
    fn from(selector: Selector) -> Self {
        Self::new(selector)
    }
}

/// Endpoint selector alias preserved for compatibility with existing benchmarks.
pub type EndpointSelector = Selector;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn selector_matches_exact_labels() {
        let selector = Selector::empty().with_label("app", "frontend");
        let labels = HashMap::from([
            ("app".to_string(), "frontend".to_string()),
            ("tier".to_string(), "web".to_string()),
        ]);

        assert!(selector.matches(&labels));
    }

    #[test]
    fn selector_matches_requirements() {
        let selector = Selector::empty()
            .with_requirement(Requirement::new(
                "env",
                RequirementOperator::In,
                ["prod", "staging"],
            ))
            .with_requirement(Requirement::new(
                "debug",
                RequirementOperator::DoesNotExist,
                std::iter::empty::<String>(),
            ));
        let labels = HashMap::from([("env".to_string(), "prod".to_string())]);

        assert!(selector.matches(&labels));
    }

    #[test]
    fn cached_selector_uses_stable_key() {
        let selector = CachedSelector::new(Selector::empty().with_label("app", "web"));
        assert!(selector.key().contains("app=web"));
    }
}
