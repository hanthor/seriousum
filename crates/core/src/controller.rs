//! Controller system for seriousum.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Failed { failures: u32 },
    Stopped,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => f.write_str("running"),
            Self::Failed { failures } => write!(f, "failed ({failures} failures)"),
            Self::Stopped => f.write_str("stopped"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub group: String,
    pub name: String,
    pub rate_limit: Option<Duration>,
}

impl Config {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            group: String::new(),
            name: name.into(),
            rate_limit: None,
        }
    }
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
    pub fn with_rate_limit(mut self, duration: Duration) -> Self {
        self.rate_limit = Some(duration);
        self
    }
}

pub type WorkerFn =
    Box<dyn FnMut() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send>;

#[derive(Clone)]
pub struct Controller {
    pub config: Arc<Config>,
    pub status: Arc<RwLock<Status>>,
    worker: Arc<RwLock<Option<WorkerFn>>>,
}

impl Controller {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            status: Arc::new(RwLock::new(Status::Stopped)),
            worker: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_worker<F, Fut>(&self, mut worker: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        *self.worker.write().await = Some(Box::new(move || Box::pin(worker())));
    }

    pub async fn run_once(&self) -> anyhow::Result<()> {
        *self.status.write().await = Status::Running;
        let mut guard = self.worker.write().await;
        if let Some(worker) = guard.as_mut() {
            worker().await?;
        }
        Ok(())
    }

    pub async fn stop(&self) {
        *self.status.write().await = Status::Stopped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn controller_basic() {
        let controller = Controller::new(Config::new("x"));
        controller.set_worker(|| async { Ok(()) }).await;
        controller.run_once().await.unwrap();
        assert_eq!(*controller.status.read().await, Status::Running);
        controller.stop().await;
        assert_eq!(*controller.status.read().await, Status::Stopped);
    }
}
