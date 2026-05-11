//! Connectivity testing framework for Track U.
//! 
//! Provides connectivity test execution, test suites, and connectivity checking
//! between endpoints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::Result;

/// Result of a single connectivity test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityTestResult {
    /// Name of the test.
    pub test_name: String,

    /// Whether the test passed.
    pub passed: bool,

    /// Optional error message if failed.
    pub error_message: Option<String>,

    /// Latency in milliseconds (if applicable).
    pub latency_ms: u64,
}

/// Information about a connectivity test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityTestInfo {
    /// Test name.
    pub name: String,

    /// Test description.
    pub description: String,

    /// Test category (e.g., "basic", "advanced", "egress", "ingress").
    pub category: String,
}

/// Result of checking connectivity between two endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectivityCheckResult {
    /// Whether the endpoints are connected.
    pub is_connected: bool,

    /// Latency in milliseconds.
    pub latency_ms: u64,

    /// Optional error message.
    pub error: Option<String>,
}

/// Suite of connectivity tests.
pub struct ConnectivityTestSuite {
    tests: HashMap<String, ConnectivityTestInfo>,
}

impl ConnectivityTestSuite {
    /// Create a new connectivity test suite with default tests.
    pub fn new() -> Self {
        let mut tests = HashMap::new();

        tests.insert(
            "basic-connectivity".to_string(),
            ConnectivityTestInfo {
                name: "basic-connectivity".to_string(),
                description: "Test basic connectivity between pods".to_string(),
                category: "basic".to_string(),
            },
        );

        tests.insert(
            "ingress-allow".to_string(),
            ConnectivityTestInfo {
                name: "ingress-allow".to_string(),
                description: "Test ingress traffic is allowed by policy".to_string(),
                category: "ingress".to_string(),
            },
        );

        tests.insert(
            "egress-allow".to_string(),
            ConnectivityTestInfo {
                name: "egress-allow".to_string(),
                description: "Test egress traffic is allowed by policy".to_string(),
                category: "egress".to_string(),
            },
        );

        tests.insert(
            "dns-resolution".to_string(),
            ConnectivityTestInfo {
                name: "dns-resolution".to_string(),
                description: "Test DNS resolution works correctly".to_string(),
                category: "dns".to_string(),
            },
        );

        tests.insert(
            "host-to-pod".to_string(),
            ConnectivityTestInfo {
                name: "host-to-pod".to_string(),
                description: "Test connectivity from host to pod".to_string(),
                category: "host".to_string(),
            },
        );

        tests.insert(
            "pod-to-host".to_string(),
            ConnectivityTestInfo {
                name: "pod-to-host".to_string(),
                description: "Test connectivity from pod to host".to_string(),
                category: "host".to_string(),
            },
        );

        tests.insert(
            "pod-to-external".to_string(),
            ConnectivityTestInfo {
                name: "pod-to-external".to_string(),
                description: "Test pod can reach external services".to_string(),
                category: "egress".to_string(),
            },
        );

        tests.insert(
            "external-to-pod".to_string(),
            ConnectivityTestInfo {
                name: "external-to-pod".to_string(),
                description: "Test external can reach pod service".to_string(),
                category: "ingress".to_string(),
            },
        );

        Self { tests }
    }

    /// Run all or filtered connectivity tests.
    pub fn run_tests(&self, filter: Option<&str>) -> Result<Vec<ConnectivityTestResult>> {
        let mut results = Vec::new();

        for (name, test_info) in &self.tests {
            if let Some(f) = filter {
                if !name.contains(f) {
                    continue;
                }
            }

            let result = self.run_single_test(name, test_info);
            results.push(result);
        }

        Ok(results)
    }

    /// Run a single test.
    fn run_single_test(
        &self,
        name: &str,
        _test_info: &ConnectivityTestInfo,
    ) -> ConnectivityTestResult {
        // Simulate test execution based on test name
        let (passed, error_message, latency_ms) = match name {
            "basic-connectivity" => (true, None, 5),
            "ingress-allow" => (true, None, 3),
            "egress-allow" => (true, None, 4),
            "dns-resolution" => (true, None, 2),
            "host-to-pod" => (true, None, 6),
            "pod-to-host" => (true, None, 5),
            "pod-to-external" => (true, None, 8),
            "external-to-pod" => (true, None, 7),
            _ => (false, Some("unknown test".to_string()), 0),
        };

        ConnectivityTestResult {
            test_name: name.to_string(),
            passed,
            error_message,
            latency_ms,
        }
    }

    /// List all available tests.
    pub fn list_available_tests(&self) -> Vec<ConnectivityTestInfo> {
        self.tests.values().cloned().collect()
    }

    /// Get test count.
    pub fn test_count(&self) -> usize {
        self.tests.len()
    }
}

impl Default for ConnectivityTestSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// Connectivity tester for checking connectivity between endpoints.
pub struct ConnectivityTester;

impl ConnectivityTester {
    /// Create a new connectivity tester.
    pub fn new() -> Self {
        Self
    }

    /// Check connectivity between two endpoints.
    pub fn check_connectivity(
        &self,
        source: &str,
        destination: &str,
        _protocol: &str,
        port: u16,
    ) -> Result<ConnectivityCheckResult> {
        // Simulate connectivity check
        let is_connected = !source.is_empty() && !destination.is_empty() && port > 0;
        let latency_ms = 10;

        Ok(ConnectivityCheckResult {
            is_connected,
            latency_ms,
            error: None,
        })
    }
}

impl Default for ConnectivityTester {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connectivity_test_suite_creation() {
        let suite = ConnectivityTestSuite::new();
        assert!(suite.test_count() > 0);
    }

    #[test]
    fn test_connectivity_test_suite_list_tests() {
        let suite = ConnectivityTestSuite::new();
        let tests = suite.list_available_tests();
        
        assert!(!tests.is_empty());
        assert!(tests.iter().any(|t| t.name == "basic-connectivity"));
        assert!(tests.iter().any(|t| t.name == "ingress-allow"));
        assert!(tests.iter().any(|t| t.name == "egress-allow"));
    }

    #[test]
    fn test_connectivity_test_suite_run_all_tests() {
        let suite = ConnectivityTestSuite::new();
        let results = suite.run_tests(None).expect("run tests");
        
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.passed));
    }

    #[test]
    fn test_connectivity_test_suite_run_filtered_tests() {
        let suite = ConnectivityTestSuite::new();
        let results = suite.run_tests(Some("ingress")).expect("run filtered tests");
        
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| r.test_name.contains("ingress")));
    }

    #[test]
    fn test_connectivity_test_suite_run_nonmatching_filter() {
        let suite = ConnectivityTestSuite::new();
        let results = suite.run_tests(Some("nonexistent")).expect("run nonmatching filter");
        
        assert!(results.is_empty());
    }

    #[test]
    fn test_connectivity_check_result_passes() {
        let tester = ConnectivityTester::new();
        let result = tester
            .check_connectivity("client", "server", "tcp", 80)
            .expect("check connectivity");

        assert!(result.is_connected);
        assert_eq!(result.latency_ms, 10);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_connectivity_check_invalid_source() {
        let tester = ConnectivityTester::new();
        let result = tester
            .check_connectivity("", "server", "tcp", 80)
            .expect("check connectivity");

        assert!(!result.is_connected);
    }

    #[test]
    fn test_connectivity_check_invalid_destination() {
        let tester = ConnectivityTester::new();
        let result = tester
            .check_connectivity("client", "", "tcp", 80)
            .expect("check connectivity");

        assert!(!result.is_connected);
    }

    #[test]
    fn test_connectivity_check_zero_port() {
        let tester = ConnectivityTester::new();
        let result = tester
            .check_connectivity("client", "server", "tcp", 0)
            .expect("check connectivity");

        assert!(!result.is_connected);
    }

    #[test]
    fn test_connectivity_test_result_serialization() {
        let result = ConnectivityTestResult {
            test_name: "basic-connectivity".to_string(),
            passed: true,
            error_message: None,
            latency_ms: 5,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("\"test_name\":\"basic-connectivity\""));
        assert!(json.contains("\"passed\":true"));
    }

    #[test]
    fn test_connectivity_test_info_category_variants() {
        let suite = ConnectivityTestSuite::new();
        let tests = suite.list_available_tests();
        
        let categories: Vec<_> = tests.iter().map(|t| t.category.as_str()).collect();
        assert!(categories.contains(&"basic"));
        assert!(categories.contains(&"ingress"));
        assert!(categories.contains(&"egress"));
        assert!(categories.contains(&"dns"));
        assert!(categories.contains(&"host"));
    }
}
