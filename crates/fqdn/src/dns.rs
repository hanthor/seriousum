//! DNS message parsing and handling
//!
//! Basic DNS packet parsing for extracting questions from DNS queries.

use serde::{Deserialize, Serialize};

/// DNS question section
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsQuestionSection {
    /// DNS name being queried
    pub name: String,

    /// DNS record type (A, AAAA, CNAME, etc.)
    pub record_type: u16,

    /// DNS class (usually IN for Internet)
    pub class: u16,
}

impl DnsQuestionSection {
    /// Creates a new DNS question
    pub fn new(name: impl Into<String>, record_type: u16, class: u16) -> Self {
        Self {
            name: name.into(),
            record_type,
            class,
        }
    }

    /// Checks if this is an A record query (IPv4)
    pub fn is_a_record(&self) -> bool {
        self.record_type == 1
    }

    /// Checks if this is an AAAA record query (IPv6)
    pub fn is_aaaa_record(&self) -> bool {
        self.record_type == 28
    }

    /// Checks if this is a CNAME record query
    pub fn is_cname_record(&self) -> bool {
        self.record_type == 5
    }

    /// Checks if this is an internet class query
    pub fn is_internet_class(&self) -> bool {
        self.class == 1
    }
}

/// Parsed DNS message (query/response)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsMessage {
    /// Message ID for matching requests/responses
    pub id: u16,

    /// Questions section
    pub questions: Vec<DnsQuestionSection>,

    /// Whether this is a query (false) or response (true)
    pub is_response: bool,

    /// Whether recursion is desired
    pub recursion_desired: bool,

    /// Whether this is an authoritative answer
    pub authoritative_answer: bool,
}

impl DnsMessage {
    /// Creates a new DNS message
    pub fn new(id: u16, is_response: bool) -> Self {
        Self {
            id,
            questions: Vec::new(),
            is_response,
            recursion_desired: false,
            authoritative_answer: false,
        }
    }

    /// Adds a question to the message
    pub fn add_question(mut self, question: DnsQuestionSection) -> Self {
        self.questions.push(question);
        self
    }

    /// Checks if this message has any A or AAAA record questions
    pub fn has_ip_record_questions(&self) -> bool {
        self.questions
            .iter()
            .any(|q| q.is_a_record() || q.is_aaaa_record())
    }

    /// Gets all names being queried for A or AAAA records
    pub fn get_queried_names(&self) -> Vec<String> {
        self.questions
            .iter()
            .filter(|q| (q.is_a_record() || q.is_aaaa_record()) && q.is_internet_class())
            .map(|q| q.name.clone())
            .collect()
    }
}

/// Normalizes DNS name (lowercase, adds trailing dot).
///
/// Uses the full `is_fqdn` semantics (backslash-aware). Equivalent to
/// `dns.FQDN` from `cilium/pkg/fqdn/dns`.
pub fn normalize_fqdn(name: &str) -> String {
    fqdn(name)
}

/// Reports whether the domain name s is fully qualified.
///
/// A name is FQDN if it ends with an unescaped `.`. The trailing dot is
/// unescaped when it is not preceded by an odd number of backslashes.
///
/// Ported from `cilium/pkg/fqdn/dns/dns.go` (originally from `github.com/miekg/dns`).
pub fn is_fqdn(s: &str) -> bool {
    // Must end with '.'
    if s.is_empty() || !s.ends_with('.') {
        return false;
    }
    // Strip the trailing dot
    let s = &s[..s.len() - 1];

    // If there's no backslash immediately before the dot, it's unescaped.
    if s.is_empty() || !s.ends_with('\\') {
        return true;
    }

    // Count the number of consecutive backslashes before the dot.
    // An even count means the backslashes escape each other (dot is unescaped).
    // An odd count means the last backslash escapes the dot.
    let backslash_count = s.chars().rev().take_while(|&c| c == '\\').count();
    backslash_count % 2 == 0
}

/// Returns the fully qualified domain name from s.
///
/// If s is already fully qualified, it is returned (lowercased) unchanged.
/// Otherwise a trailing `.` is appended.
///
/// Ported from `cilium/pkg/fqdn/dns/dns.go`.
pub fn fqdn(s: &str) -> String {
    if is_fqdn(s) {
        s.to_lowercase()
    } else {
        format!("{}.", s.to_lowercase())
    }
}

/// Checks if a name is fully qualified (ends with .).
///
/// Note: this is a simplified check that does not account for backslash
/// escaping. Use `is_fqdn` for the full semantics from `cilium/pkg/fqdn/dns`.
pub fn is_fqdn_simple(name: &str) -> bool {
    name.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_question_a_record() {
        let q = DnsQuestionSection::new("example.com", 1, 1);
        assert!(q.is_a_record());
        assert!(!q.is_aaaa_record());
        assert!(q.is_internet_class());
    }

    #[test]
    fn dns_question_aaaa_record() {
        let q = DnsQuestionSection::new("example.com", 28, 1);
        assert!(q.is_aaaa_record());
        assert!(!q.is_a_record());
    }

    #[test]
    fn dns_message_creation() {
        let msg = DnsMessage::new(1234, false)
            .add_question(DnsQuestionSection::new("example.com", 1, 1))
            .add_question(DnsQuestionSection::new("example.org", 28, 1));

        assert_eq!(msg.id, 1234);
        assert!(!msg.is_response);
        assert_eq!(msg.questions.len(), 2);
        assert!(msg.has_ip_record_questions());
    }

    #[test]
    fn dns_message_queried_names() {
        let msg = DnsMessage::new(1, false)
            .add_question(DnsQuestionSection::new("example.com", 1, 1))
            .add_question(DnsQuestionSection::new("example.org", 28, 1));

        let names = msg.get_queried_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"example.com".to_string()));
    }

    #[test]
    fn normalize_fqdn_adds_dot() {
        assert_eq!(normalize_fqdn("example.com"), "example.com.");
        assert_eq!(normalize_fqdn("example.com."), "example.com.");
    }

    #[test]
    fn normalize_fqdn_lowercase() {
        assert_eq!(normalize_fqdn("EXAMPLE.COM"), "example.com.");
    }

    #[test]
    fn is_fqdn_check() {
        assert!(is_fqdn("example.com."));
        assert!(!is_fqdn("example.com"));
        // Escaped dot: not FQDN
        assert!(!is_fqdn("example.com\\."));
        // Double-escaped backslash + dot: FQDN
        assert!(is_fqdn("example.com\\\\."));
    }
}

#[cfg(test)]
mod parity_tests {
    //! Parity tests ported from `cilium/pkg/fqdn/dns/dns_test.go`.

    use super::*;

    /// Ported from `TestIsFQDN` in `cilium/pkg/fqdn/dns/dns_test.go`.
    #[test]
    fn test_is_fqdn() {
        let cases: &[(&str, bool)] = &[
            (".", true),
            ("\\.", false),
            ("\\\\.", true),
            ("\\\\\\.", false),
            ("\\\\\\\\.", true),
            ("a.", true),
            ("a\\.", false),
            ("a\\\\.", true),
            ("a\\\\\\.", false),
            ("ab.", true),
            ("ab\\.", false),
            ("ab\\\\.", true),
            ("ab\\\\\\.", false),
            ("..", true),
            (".\\.", false),
            (".\\\\.", true),
            (".\\\\\\.", false),
            ("example.org.", true),
            ("example.org\\.", false),
            ("example.org\\\\.", true),
            ("example.org\\\\\\.", false),
            ("example\\.org.", true),
            ("example\\\\.org.", true),
            ("example\\\\\\.org.", true),
            ("\\example.org.", true),
            ("\\\\example.org.", true),
            ("\\\\\\example.org.", true),
        ];

        for (s, expect) in cases {
            assert_eq!(
                is_fqdn(s),
                *expect,
                "is_fqdn({s:?}) = {}, expected {}",
                is_fqdn(s),
                expect
            );
        }
    }

    /// Ported from `TestFQDN` in `cilium/pkg/fqdn/dns/dns_test.go`.
    #[test]
    fn test_fqdn() {
        let cases: &[(&str, &str)] = &[
            (".", "."),
            ("\\.", "\\.."),
            ("example.org", "example.org."),
            ("example.org.", "example.org."),
            ("example.org\\.", "example.org\\.."),
            ("example.org\\\\.", "example.org\\\\."),
            ("EXAMPLE.ORG", "example.org."),
            ("eXAMPLE.org.", "example.org."),
        ];

        for (s, expect) in cases {
            assert_eq!(
                fqdn(s),
                *expect,
                "fqdn({s:?}) = {:?}, expected {:?}",
                fqdn(s),
                expect
            );
        }
    }
}
