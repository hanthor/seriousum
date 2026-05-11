//! Policy selectors for endpoint/identity matching

use std::collections::HashMap;

use crate::EndpointIdentity;

/// Endpoint selector: matches by labels
#[derive(Debug, Clone)]
pub struct EndpointSelector {
    pub labels: HashMap<String, String>,
}

impl EndpointSelector {
    pub fn new(labels: HashMap<String, String>) -> Self {
        Self { labels }
    }

    pub fn empty() -> Self {
        Self {
            labels: HashMap::new(),
        }
    }

    /// Match all endpoints (wildcard)
    pub fn match_all() -> Self {
        Self::empty()
    }

    /// Check if this selector matches the given labels
    pub fn matches(&self, endpoint_labels: &HashMap<String, String>) -> bool {
        if self.labels.is_empty() {
            // Empty selector matches all
            return true;
        }

        // All selector labels must match endpoint labels
        self.labels
            .iter()
            .all(|(key, value)| endpoint_labels.get(key) == Some(value))
    }

    /// Add a label constraint
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Generic selector for matching
#[derive(Debug, Clone)]
pub enum Selector {
    /// Match specific identity
    Identity(EndpointIdentity),
    /// Match by endpoint labels
    Labels(EndpointSelector),
    /// Match any (wildcard)
    Any,
}

impl Selector {
    pub fn identity(id: EndpointIdentity) -> Self {
        Self::Identity(id)
    }

    pub fn labels(labels: HashMap<String, String>) -> Self {
        Self::Labels(EndpointSelector::new(labels))
    }

    pub fn any() -> Self {
        Self::Any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_selector_empty() {
        let sel = EndpointSelector::empty();
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        assert!(sel.matches(&labels));
    }

    #[test]
    fn test_endpoint_selector_match_all() {
        let sel = EndpointSelector::match_all();
        let labels = HashMap::new();
        assert!(sel.matches(&labels));
    }

    #[test]
    fn test_endpoint_selector_with_label() {
        let mut expected = HashMap::new();
        expected.insert("app".to_string(), "web".to_string());

        let sel = EndpointSelector::new(expected.clone());

        let mut endpoint_labels = HashMap::new();
        endpoint_labels.insert("app".to_string(), "web".to_string());
        endpoint_labels.insert("tier".to_string(), "frontend".to_string());

        assert!(sel.matches(&endpoint_labels));
    }

    #[test]
    fn test_endpoint_selector_no_match() {
        let mut expected = HashMap::new();
        expected.insert("app".to_string(), "web".to_string());

        let sel = EndpointSelector::new(expected);

        let mut endpoint_labels = HashMap::new();
        endpoint_labels.insert("app".to_string(), "api".to_string());

        assert!(!sel.matches(&endpoint_labels));
    }

    #[test]
    fn test_endpoint_selector_fluent() {
        let sel = EndpointSelector::empty()
            .with_label("app", "web")
            .with_label("tier", "frontend");

        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("tier".to_string(), "frontend".to_string());

        assert!(sel.matches(&labels));
    }

    #[test]
    fn test_selector_identity() {
        let sel = Selector::identity(EndpointIdentity::WORLD);
        assert!(matches!(sel, Selector::Identity(_)));
    }

    #[test]
    fn test_selector_any() {
        let sel = Selector::any();
        assert!(matches!(sel, Selector::Any));
    }
}
