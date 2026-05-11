//! Error types for seriousum.

/// A result type alias using seriousum's Error type.
pub type Result<T> = std::result::Result<T, Error>;

/// The primary error type for seriousum.
///
/// This enum covers all error domains in the system: eBPF operations,
/// network operations, Kubernetes API calls, policy evaluation, and more.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A generic error with a message.
    #[error(transparent)]
    Generic(#[from] anyhow::Error),

    /// An I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An eBPF-related error.
    #[error("eBPF error: {0}")]
    Ebpf(String),

    /// A network-related error.
    #[error("network error: {0}")]
    Network(String),

    /// A Kubernetes API error.
    #[error("kubernetes error: {0}")]
    K8s(String),

    /// A policy evaluation error.
    #[error("policy error: {0}")]
    Policy(String),

    /// A configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// An endpoint-related error.
    #[error("endpoint error: {0}")]
    Endpoint(String),

    /// An identity-related error.
    #[error("identity error: {0}")]
    Identity(String),

    /// A loadbalancer-related error.
    #[error("loadbalancer error: {0}")]
    Loadbalancer(String),

    /// An IPAM-related error.
    #[error("IPAM error: {0}")]
    Ipam(String),

    /// A node-related error.
    #[error("node error: {0}")]
    Node(String),

    /// A clustermesh-related error.
    #[error("clustermesh error: {0}")]
    Clustermesh(String),

    /// A BGP-related error.
    #[error("BGP error: {0}")]
    Bgp(String),

    /// An authentication/authorization error.
    #[error("auth error: {0}")]
    Auth(String),

    /// A proxy-related error.
    #[error("proxy error: {0}")]
    Proxy(String),

    /// A wireguard-related error.
    #[error("wireguard error: {0}")]
    Wireguard(String),

    /// A crypto-related error.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// A kvstore-related error.
    #[error("kvstore error: {0}")]
    Kvstore(String),

    /// A controller-related error.
    #[error("controller error: {0}")]
    Controller(String),

    /// A metrics-related error.
    #[error("metrics error: {0}")]
    Metrics(String),

    /// A hubble-related error.
    #[error("hubble error: {0}")]
    Hubble(String),

    /// An API-related error.
    #[error("API error: {0}")]
    Api(String),

    /// A CNI-related error.
    #[error("CNI error: {0}")]
    Cni(String),

    /// An FQDN-related error.
    #[error("FQDN error: {0}")]
    Fqdn(String),

    /// An envoy-related error.
    #[error("envoy error: {0}")]
    Envoy(String),

    /// A monitor-related error.
    #[error("monitor error: {0}")]
    Monitor(String),
}

impl Error {
    /// Returns true if this is an eBPF-related error.
    pub fn is_ebpf(&self) -> bool {
        matches!(self, Error::Ebpf(_))
    }

    /// Returns true if this is a network-related error.
    pub fn is_network(&self) -> bool {
        matches!(self, Error::Network(_))
    }

    /// Returns true if this is a Kubernetes-related error.
    pub fn is_k8s(&self) -> bool {
        matches!(self, Error::K8s(_))
    }

    /// Returns true if this is a policy-related error.
    pub fn is_policy(&self) -> bool {
        matches!(self, Error::Policy(_))
    }
}
