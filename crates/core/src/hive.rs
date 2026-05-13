use std::error::Error as StdError;
use std::sync::Mutex;

use tokio::sync::watch;

/// Shared error type used by minimal hive hooks.
pub type HookError = Box<dyn StdError + Send + Sync>;

/// Minimal lifecycle hook abstraction inspired by Cilium's `cell.HookInterface`.
pub trait Hook: Send + Sync {
    /// Starts the hook.
    fn start(&self) -> Result<(), HookError>;

    /// Stops the hook.
    fn stop(&self) -> Result<(), HookError>;
}

/// Minimal lifecycle manager for parity tests.
pub struct Lifecycle {
    hooks: Vec<Box<dyn Hook>>,
    started: Mutex<usize>,
}

impl Lifecycle {
    /// Creates an empty lifecycle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            started: Mutex::new(0),
        }
    }

    /// Appends a hook to the lifecycle.
    pub fn append<H: Hook + 'static>(&mut self, hook: H) {
        self.hooks.push(Box::new(hook));
    }

    /// Starts all hooks in registration order.
    pub fn start_all(&self) -> Result<(), HookError> {
        let mut started = lock(&self.started);
        while *started < self.hooks.len() {
            if let Err(error) = self.hooks[*started].start() {
                while *started > 0 {
                    *started -= 1;
                    let _ = self.hooks[*started].stop();
                }
                return Err(error);
            }
            *started += 1;
        }
        Ok(())
    }

    /// Stops all started hooks in reverse order, ignoring shutdown errors.
    pub fn stop_all(&self) {
        let mut started = lock(&self.started);
        while *started > 0 {
            *started -= 1;
            let _ = self.hooks[*started].stop();
        }
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience hook wrapper for start/stop closures.
pub struct HookFn<S, T> {
    start_fn: S,
    stop_fn: T,
}

impl<S, T> HookFn<S, T>
where
    S: Fn() -> Result<(), HookError> + Send + Sync,
    T: Fn() -> Result<(), HookError> + Send + Sync,
{
    /// Creates a hook from start and stop closures.
    #[must_use]
    pub fn new(start: S, stop: T) -> Self {
        Self {
            start_fn: start,
            stop_fn: stop,
        }
    }
}

impl<S, T> Hook for HookFn<S, T>
where
    S: Fn() -> Result<(), HookError> + Send + Sync,
    T: Fn() -> Result<(), HookError> + Send + Sync,
{
    fn start(&self) -> Result<(), HookError> {
        (self.start_fn)()
    }

    fn stop(&self) -> Result<(), HookError> {
        (self.stop_fn)()
    }
}

/// Minimal promise backed by a Tokio watch channel.
#[derive(Clone)]
pub struct Promise<T: Clone> {
    sender: watch::Sender<Option<T>>,
    receiver: watch::Receiver<Option<T>>,
}

impl<T: Clone + Send + Sync + 'static> Promise<T> {
    /// Creates an unresolved promise.
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(None);
        Self { sender, receiver }
    }

    /// Resolves the promise with a value.
    pub fn resolve(&self, value: T) {
        let _ = self.sender.send(Some(value));
    }

    /// Returns a cloned receiver for observing the promise state.
    #[must_use]
    pub fn receiver(&self) -> watch::Receiver<Option<T>> {
        self.receiver.clone()
    }

    /// Waits until the promise is resolved.
    pub async fn await_value(&mut self) -> T {
        loop {
            if let Some(value) = self.receiver.borrow().clone() {
                return value;
            }
            if self.receiver.changed().await.is_err() {
                if let Some(value) = self.receiver.borrow().clone() {
                    return value;
                }
                panic!("promise sender dropped before resolution");
            }
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Default for Promise<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn lifecycle_starts_and_stops_in_order() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut lifecycle = Lifecycle::new();

        for name in ["first", "second"] {
            let events_for_start = Arc::clone(&events);
            let events_for_stop = Arc::clone(&events);
            lifecycle.append(HookFn::new(
                move || {
                    lock(&events_for_start).push(format!("start:{name}"));
                    Ok(())
                },
                move || {
                    lock(&events_for_stop).push(format!("stop:{name}"));
                    Ok(())
                },
            ));
        }

        lifecycle.start_all().expect("lifecycle should start");
        lifecycle.stop_all();

        assert_eq!(
            *lock(&events),
            vec![
                String::from("start:first"),
                String::from("start:second"),
                String::from("stop:second"),
                String::from("stop:first"),
            ]
        );
    }

    #[tokio::test]
    async fn promise_awaits_resolution() {
        let promise = Promise::new();
        let mut waiter = promise.clone();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_task = Arc::clone(&counter);

        tokio::spawn(async move {
            counter_for_task.store(1, Ordering::SeqCst);
            promise.resolve(String::from("ready"));
        })
        .await
        .expect("task should finish");

        assert_eq!(waiter.await_value().await, "ready");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
