//! Output formatting utilities for the cilium-dbg CLI
//!
//! Provides multiple output formats:
//! - Table output with tabwriter
//! - JSON output with serde_json
//! - Compact/text output

use crate::{ConnectionTrackingEntry, Endpoint, PolicyEntry, Service};
use serde_json::json;
use std::collections::HashMap;
use tracing::info;

/// Output format for command results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable table format
    #[default]
    Table,
    /// JSON format
    Json,
    /// Compact text format
    Text,
}

/// Table printer with header and rows
pub struct TablePrinter {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TablePrinter {
    /// Create a new table with headers
    pub fn new(headers: Vec<&str>) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    /// Add a row to the table
    pub fn add_row(&mut self, row: Vec<&str>) {
        self.rows.push(row.iter().map(|r| r.to_string()).collect());
    }

    /// Print the table to stdout
    pub fn print(&self) {
        if self.rows.is_empty() {
            info!("No entries found");
            return;
        }

        // Calculate column widths
        let mut widths = vec![0; self.headers.len()];
        for (i, header) in self.headers.iter().enumerate() {
            widths[i] = header.len();
        }

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Print header
        for (i, header) in self.headers.iter().enumerate() {
            print!("{:<width$}  ", header, width = widths[i]);
        }
        info!("");

        // Print separator
        for (i, _) in self.headers.iter().enumerate() {
            print!("{:<width$}  ", "=".repeat(widths[i]), width = widths[i]);
        }
        info!("");

        // Print rows
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    print!("{:<width$}  ", cell, width = widths[i]);
                }
            }
            info!("");
        }
    }

    /// Print as JSON
    pub fn print_json(&self) -> serde_json::Result<String> {
        let mut data = Vec::new();
        for row in &self.rows {
            let mut obj = serde_json::Map::new();
            for (i, header) in self.headers.iter().enumerate() {
                let value = row.get(i).cloned().unwrap_or_default();
                obj.insert(header.clone(), json!(value));
            }
            data.push(serde_json::Value::Object(obj));
        }
        serde_json::to_string_pretty(&serde_json::json!(data))
    }
}

/// Print endpoints as a table
pub fn print_endpoints_table(endpoints: &[Endpoint]) {
    let mut table = TablePrinter::new(vec!["ID", "IPv4", "IPv6", "Identity", "State", "Labels"]);

    for ep in endpoints {
        let ipv4 = ep.ipv4.map(|ip| ip.to_string()).unwrap_or_default();
        let ipv6 = ep.ipv6.map(|ip| ip.to_string()).unwrap_or_default();
        let identity = ep
            .identity
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());
        let labels = ep
            .labels
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        table.add_row(vec![
            &ep.id.to_string(),
            &ipv4,
            &ipv6,
            &identity,
            &ep.state,
            &labels,
        ]);
    }

    table.print();
}

/// Print endpoints as JSON
pub fn print_endpoints_json(endpoints: &[Endpoint]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(endpoints)
}

/// Print services as a table
pub fn print_services_table(services: &[Service]) {
    let mut table = TablePrinter::new(vec!["ID", "Frontend", "Type", "Backends"]);

    for svc in services {
        let backends_str = if svc.backends.is_empty() {
            "-".to_string()
        } else {
            svc.backends
                .iter()
                .enumerate()
                .map(|(i, b)| format!("{}: {} ({})", i + 1, b.address, b.state))
                .collect::<Vec<_>>()
                .join("; ")
        };

        table.add_row(vec![
            &svc.id.to_string(),
            &svc.frontend,
            &svc.service_type,
            &backends_str,
        ]);
    }

    table.print();
}

/// Print services as JSON
pub fn print_services_json(services: &[Service]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(services)
}

/// Print policy entries as a table
pub fn print_policies_table(policies: &[PolicyEntry]) {
    let mut table = TablePrinter::new(vec![
        "Policy",
        "Direction",
        "Identity",
        "Port/Protocol",
        "Proxy",
        "Bytes",
        "Packets",
        "Deny",
    ]);

    for policy in policies {
        let port_proto = if policy.port == 0 {
            "any".to_string()
        } else {
            format!("{}/{}", policy.port, policy.protocol)
        };

        let proxy_port = if policy.proxy_port == 0 {
            "NONE".to_string()
        } else {
            policy.proxy_port.to_string()
        };

        table.add_row(vec![
            &policy.policy_id.to_string(),
            &policy.traffic_direction.to_string(),
            &policy.identity.to_string(),
            &port_proto,
            &proxy_port,
            &policy.bytes.to_string(),
            &policy.packets.to_string(),
            if policy.is_deny { "Yes" } else { "No" },
        ]);
    }

    table.print();
}

/// Print policy entries as JSON
pub fn print_policies_json(policies: &[PolicyEntry]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(policies)
}

/// Print connection tracking entries as a table
pub fn print_ct_entries_table(entries: &[ConnectionTrackingEntry]) {
    let mut table = TablePrinter::new(vec![
        "Source",
        "Dest",
        "Protocol",
        "State",
        "Bytes Sent",
        "Bytes Received",
    ]);

    for entry in entries {
        let source = format!("{}:{}", entry.source_ip, entry.source_port);
        let dest = format!("{}:{}", entry.dest_ip, entry.dest_port);

        table.add_row(vec![
            &source,
            &dest,
            &entry.protocol,
            &entry.state,
            &entry.bytes_sent.to_string(),
            &entry.bytes_received.to_string(),
        ]);
    }

    table.print();
}

/// Print connection tracking entries as JSON
pub fn print_ct_entries_json(entries: &[ConnectionTrackingEntry]) -> serde_json::Result<String> {
    serde_json::to_string_pretty(entries)
}

/// Print a map as a key-value table
pub fn print_map_table(data: &HashMap<String, Vec<String>>, key_title: &str, value_title: &str) {
    if data.is_empty() {
        info!("No entries found");
        return;
    }

    let mut table = TablePrinter::new(vec![key_title, value_title]);

    for (key, values) in data {
        for (i, value) in values.iter().enumerate() {
            if i == 0 {
                table.add_row(vec![key, value]);
            } else {
                table.add_row(vec!["", value]);
            }
        }
    }

    table.print();
}

/// Print a map as JSON
pub fn print_map_json(data: &HashMap<String, Vec<String>>) -> serde_json::Result<String> {
    serde_json::to_string_pretty(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrafficDirection;

    #[test]
    fn test_table_printer_creation() {
        let table = TablePrinter::new(vec!["ID", "Name", "Status"]);
        assert_eq!(table.headers.len(), 3);
        assert!(table.rows.is_empty());
    }

    #[test]
    fn test_table_printer_add_row() {
        let mut table = TablePrinter::new(vec!["ID", "Name"]);
        table.add_row(vec!["1", "test"]);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "1");
    }

    #[test]
    fn test_table_printer_json_output() {
        let mut table = TablePrinter::new(vec!["ID", "Name"]);
        table.add_row(vec!["1", "test"]);
        let json = table.print_json().unwrap();
        assert!(json.contains("ID"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_print_map_table_empty() {
        let data: HashMap<String, Vec<String>> = HashMap::new();
        print_map_table(&data, "Key", "Value");
        // Just verify it doesn't panic
    }

    #[test]
    fn test_endpoints_json_output() {
        let endpoints = vec![Endpoint::new(crate::EndpointId(1))];
        let json = print_endpoints_json(&endpoints).unwrap();
        assert!(json.contains("1"));
    }

    #[test]
    fn test_services_json_output() {
        let services = vec![Service {
            id: crate::ServiceId(1),
            frontend: "10.0.0.1:80".to_string(),
            service_type: "ClusterIP".to_string(),
            backends: vec![],
        }];
        let json = print_services_json(&services).unwrap();
        assert!(json.contains("10.0.0.1:80"));
    }

    #[test]
    fn test_policies_json_output() {
        let policies = vec![PolicyEntry {
            policy_id: 1,
            traffic_direction: TrafficDirection::Ingress,
            identity: crate::NumericIdentity::WORLD,
            port: 80,
            protocol: "tcp".to_string(),
            proxy_port: 0,
            bytes: 1000,
            packets: 50,
            is_deny: false,
        }];
        let json = print_policies_json(&policies).unwrap();
        assert!(json.contains("INGRESS"));
    }

    #[test]
    fn test_ct_entries_json_output() {
        let entries = vec![ConnectionTrackingEntry {
            source_ip: "10.0.0.1".to_string(),
            dest_ip: "10.0.0.2".to_string(),
            source_port: 12345,
            dest_port: 80,
            protocol: "tcp".to_string(),
            state: "ESTABLISHED".to_string(),
            bytes_sent: 5000,
            bytes_received: 10000,
        }];
        let json = print_ct_entries_json(&entries).unwrap();
        assert!(json.contains("ESTABLISHED"));
    }
}
