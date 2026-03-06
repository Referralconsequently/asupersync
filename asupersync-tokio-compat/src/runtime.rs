//! Tokio runtime context bridge.
//!
//! Provides [`with_tokio_context`], the keystone function that drives
//! Tokio-locked futures on a private Tokio runtime while keeping Asupersync
//! as the outer orchestration runtime.
//!
//! # Problem
//!
//! Crates like `reqwest`, `aws-sdk-s3`, and `sqlx` internally call
//! `tokio::runtime::Handle::current()`. Without a Tokio runtime context
//! on the thread-local, this panics. Merely entering a Tokio handle context
//! is not sufficient though: futures that use `tokio::spawn`, Tokio timers,
//! or other Tokio driver-backed facilities need an actual Tokio runtime to
//! poll them. This module therefore runs such futures inside a cached private
//! current-thread Tokio runtime on an Asupersync blocking thread.
//!
//! # Example
//!
//! ```ignore
//! use asupersync_tokio_compat::runtime::with_tokio_context;
//!
//! async fn handler(cx: &asupersync::Cx) {
//!     let body = with_tokio_context(cx, || async {
//!         reqwest::get("https://example.com").await?.text().await
//!     }).await;
//! }
//! ```

use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    /// Cached Tokio runtime for the current thread. Creating a Tokio
    /// runtime is cheap but not free; reusing one avoids repeated setup.
    static TOKIO_RT: RefCell<Option<Arc<tokio::runtime::Runtime>>> = const { RefCell::new(None) };
}

/// Get or create a thread-local Tokio current-thread runtime.
fn get_or_create_tokio_rt() -> Arc<tokio::runtime::Runtime> {
    TOKIO_RT.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(rt) = borrow.as_ref() {
            return Arc::clone(rt);
        }
        let rt = Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create Tokio compatibility runtime"),
        );
        *borrow = Some(Arc::clone(&rt));
        rt
    })
}

/// Execute an async closure inside a Tokio runtime context.
///
/// This function:
/// 1. Uses the Asupersync blocking pool to avoid blocking the caller's async
///    thread while the Tokio runtime drives the future.
/// 2. Creates (or reuses) a private current-thread Tokio runtime on that
///    blocking thread.
/// 3. Creates the closure's future inside `Runtime::block_on`, so both
///    future construction and polling happen with a valid Tokio context.
/// 4. Runs the closure's future with `Runtime::block_on`, which also drives
///    Tokio tasks, timers, and other driver-backed facilities used by the
///    wrapped future.
///
/// # Returns
///
/// - `Some(T)` if the future completed and the `Cx` was not cancelled.
/// - `None` if the `Cx` was already cancelled before the call or became
///   cancelled during execution.
///
/// # Panics
///
/// Panics if a Tokio runtime cannot be created (should not happen with
/// the `rt` feature enabled).
#[allow(clippy::future_not_send)] // EnterGuard is !Send by design
pub async fn with_tokio_context<F, Fut, T>(cx: &asupersync::Cx, f: F) -> Option<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + 'static,
    T: Send + 'static,
{
    // Bail early if already cancelled.
    if cx.is_cancel_requested() {
        return None;
    }

    match crate::blocking::block_on_sync(
        cx,
        move || {
            let rt = get_or_create_tokio_rt();
            rt.block_on(async move { f().await })
        },
        crate::CancellationMode::Strict,
    )
    .await
    {
        crate::blocking::BlockingOutcome::Ok(value) => Some(value),
        crate::blocking::BlockingOutcome::Cancelled => None,
        crate::blocking::BlockingOutcome::Panicked(message) => {
            panic!("Tokio compatibility future panicked: {message}")
        }
    }
}

/// Execute a synchronous closure inside a Tokio runtime context.
///
/// Useful for constructing Tokio-locked clients (e.g., `reqwest::Client::new()`)
/// that check `Handle::current()` during initialization.
///
/// # Example
///
/// ```ignore
/// use asupersync_tokio_compat::runtime::with_tokio_context_sync;
///
/// let client = with_tokio_context_sync(|| reqwest::Client::new());
/// ```
pub fn with_tokio_context_sync<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    let rt = get_or_create_tokio_rt();
    let _guard = rt.enter();
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        futures_lite::future::block_on(future)
    }

    #[test]
    fn tokio_handle_current_works_inside_context() {
        run(async {
            let cx = asupersync::Cx::for_testing();
            let result = with_tokio_context(&cx, || async {
                // This would panic without a Tokio context.
                let _handle = tokio::runtime::Handle::current();
                42
            })
            .await;
            assert_eq!(result, Some(42));
        });
    }

    #[test]
    fn returns_none_when_cx_already_cancelled() {
        run(async {
            let cx = asupersync::Cx::for_testing();
            cx.cancel_fast(asupersync::types::CancelKind::User);
            let result = with_tokio_context(&cx, || async { 42 }).await;
            assert!(result.is_none());
        });
    }

    #[test]
    fn sync_context_provides_handle() {
        let value = with_tokio_context_sync(|| {
            let _handle = tokio::runtime::Handle::current();
            99
        });
        assert_eq!(value, 99);
    }

    #[test]
    fn async_value_propagates() {
        run(async {
            let cx = asupersync::Cx::for_testing();
            let result = with_tokio_context(&cx, || async { String::from("hello") }).await;
            assert_eq!(result.as_deref(), Some("hello"));
        });
    }

    #[test]
    fn tokio_spawn_runs_inside_context() {
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let cx = asupersync::Cx::for_testing();
            let result = run(with_tokio_context(&cx, || async {
                tokio::spawn(async { 7 })
                    .await
                    .expect("Tokio task should join successfully")
            }));
            tx.send(result).expect("channel send must succeed");
        });

        let result = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("Tokio-compat future should not stall");
        assert_eq!(result, Some(7));
    }

    #[test]
    fn runtime_is_reused_on_same_thread() {
        let first = with_tokio_context_sync(get_or_create_tokio_rt);
        let second = with_tokio_context_sync(get_or_create_tokio_rt);
        assert!(Arc::ptr_eq(&first, &second));
    }
}
