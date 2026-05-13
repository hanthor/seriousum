//! FQDN DNS proxy for Cilium
//!
//! This module provides DNS query interception, caching, and FQDN-based policy enforcement.
//! Ported from `cilium/pkg/fqdn` and `cilium/pkg/fqdn/dnsproxy`.

pub mod cache;
pub mod dns;
pub mod error;
pub mod model;
pub mod policy;
pub mod proxy;
pub mod types;

pub use cache::{CacheEntry, DnsCache, UpdateStatus};
pub use dns::{DnsMessage, DnsQuestionSection};
pub use error::{Error, Result};
pub use model::{DNSCache, DNSCacheEntry, DNSPattern, FQDN, FQDNError, FQDNSelector, NameManager};
pub use policy::{FqdnPolicy, PolicySelector};
pub use proxy::DnsProxy;
pub use types::{FqdnSelector, IpCidr, NameToIp};

/// Component name for FQDN subsystem
pub const COMPONENT: &str = "seriousum-fqdn";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_name_is_set() {
        assert_eq!(COMPONENT, "seriousum-fqdn");
    }
}
