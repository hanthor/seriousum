use std::fmt;
use std::time::Duration;

pub use seriousum_core::controller::{Config as ControllerConfig, Status};

/// Convenience result type for the controller scaffold.
pub type Result<T> = anyhow::Result<T>;

/// Minimal controller scaffold built on the shared core controller.
#[derive(Clone)]
pub struct ControllerScaffold {
    controller: seriousum_core::Controller,
}

impl ControllerScaffold {
    /// Create a new scaffold from a controller name.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_config(ControllerConfig::new(name))
    }

    /// Create a new scaffold from an explicit config.
    pub fn with_config(config: ControllerConfig) -> Self {
        Self {
            controller: seriousum_core::Controller::new(config),
        }
    }

    /// Create the default scaffold.
    pub fn scaffold() -> Self {
        Self::new("seriousum-controller").with_group("controller")
    }

    /// Set the logical controller group.
    pub fn with_group(self, group: impl Into<String>) -> Self {
        let mut config = (*self.controller.config).clone();
        config.group = group.into();
        Self::with_config(config)
    }

    /// Set the controller rate limit.
    pub fn with_rate_limit(self, rate_limit: Duration) -> Self {
        let mut config = (*self.controller.config).clone();
        config.rate_limit = Some(rate_limit);
        Self::with_config(config)
    }

    /// Access the controller config.
    pub fn config(&self) -> ControllerConfig {
        (*self.controller.config).clone()
    }

    /// Access the controller status.
    pub fn status(&self) -> Status {
        self.controller
            .status
            .try_read()
            .map_or(Status::Stopped, |status| *status)
    }

    /// Delegate worker registration to the shared core controller.
    pub async fn set_worker<F, Fut>(&self, worker: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.controller.set_worker(worker).await;
    }

    /// Run the controller once.
    pub async fn run_once(&self) -> anyhow::Result<()> {
        self.controller.run_once().await
    }

    /// Stop the controller.
    pub async fn stop(&self) {
        self.controller.stop().await;
    }

    /// Build a concise report.
    pub fn report(&self) -> ControllerReport {
        let config = self.config();
        ControllerReport {
            name: config.name,
            group: config.group,
            rate_limit_ms: config
                .rate_limit
                .map(|duration| duration.as_millis() as u64),
            status: self.status(),
        }
    }

    /// Render the report as a string.
    pub fn summary(&self) -> String {
        self.report().to_string()
    }
}

/// Controller report rendered by the thin binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerReport {
    /// Controller name.
    pub name: String,
    /// Controller group.
    pub group: String,
    /// Optional rate limit in milliseconds.
    pub rate_limit_ms: Option<u64>,
    /// Current status.
    pub status: Status,
}

impl fmt::Display for ControllerReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rate_limit_ms {
            Some(rate_limit_ms) => write!(
                f,
                "controller scaffold ready | name={} | group={} | rate_limit_ms={} | status={}",
                self.name, self.group, rate_limit_ms, self.status,
            ),
            None => write!(
                f,
                "controller scaffold ready | name={} | group={} | status={}",
                self.name, self.group, self.status,
            ),
        }
    }
}

/// Run the controller scaffold.
pub fn run() -> Result<String> {
    Ok(ControllerScaffold::scaffold().summary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_uses_shared_core_controller() {
        let scaffold = ControllerScaffold::scaffold();
        let config = scaffold.config();

        assert_eq!(config.name, "seriousum-controller");
        assert_eq!(config.group, "controller");
        assert_eq!(scaffold.status(), Status::Stopped);
    }

    #[test]
    fn report_includes_rate_limit_when_present() {
        let scaffold = ControllerScaffold::new("worker")
            .with_group("dataplane")
            .with_rate_limit(Duration::from_secs(2));
        let report = scaffold.report();

        assert_eq!(report.name, "worker");
        assert_eq!(report.group, "dataplane");
        assert_eq!(report.rate_limit_ms, Some(2000));
        assert_eq!(report.status, Status::Stopped);
        assert!(report.to_string().contains("rate_limit_ms=2000"));
    }

    #[tokio::test]
    async fn controller_delegates_worker_lifecycle() {
        let scaffold = ControllerScaffold::new("worker");
        scaffold.set_worker(|| async { Ok(()) }).await;
        scaffold.run_once().await.expect("run controller once");
        assert_eq!(scaffold.status(), Status::Running);
        scaffold.stop().await;
        assert_eq!(scaffold.status(), Status::Stopped);
    }

    #[test]
    fn run_returns_summary() {
        let output = run().expect("run controller scaffold");

        assert!(output.contains("controller scaffold ready"));
        assert!(output.contains("name=seriousum-controller"));
    }
}
