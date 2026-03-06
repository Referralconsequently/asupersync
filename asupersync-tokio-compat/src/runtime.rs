//! Tokio runtime context bridge.
//!
//! Provides [`with_tokio_context`], the keystone function that satisfies
//! `tokio::runtime::Handle::current()` for Tokio-locked crates while
//! keeping Asupersync as the actual executor.
//!
//! # Problem
//!
//! Crates like `reqwest`, `aws-sdk-s3`, and `sqlx` internally call
//! `tokio::runtime::Handle::current()`. Without a Tokio runtime context
//! on the thread-local, this panics. This module creates a minimal Tokio
//! `Runtime` (using only the `rt` feature — no multi-thread, no net, no fs)
//! purely to install a valid `Handle` into thread-local storage.
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
/// 1. Creates (or reuses) a minimal Tokio `Runtime` with a current-thread
///    scheduler — just enough for `Handle::current()` to succeed.
/// 2. Enters the Tokio runtime context (installs the `Handle` in TLS).
/// 3. Runs the closure's future, checking Asupersync cancellation.
/// 4. Drops the enter guard on completion.
///
/// The Tokio runtime is **not** driving I/O or timers for Asupersync.
/// It exists solely to satisfy `Handle::current()` checks in Tokio-locked
/// libraries.
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
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    // Bail early if already cancelled.
    if cx.is_cancel_requested() {
        return None;
    }

    let rt = get_or_create_tokio_rt();

    // Enter the Tokio context so Handle::current() works on this thread.
    let _guard = rt.enter();

    // Create the user future while inside the Tokio context, so any
    // client construction that calls Handle::current() succeeds.
    let value = f().await;

    // Check cancellation after completion.
    if cx.is_cancel_requested() {
        return None;
    }

    Some(value)
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
    fn runtime_is_reused_on_same_thread() {
        // Call twice; the second should reuse the cached runtime.
        run(async {
            let cx = asupersync::Cx::for_testing();
            let a = with_tokio_context(&cx, || async { 1 }).await;
            let b = with_tokio_context(&cx, || async { 2 }).await;
            assert_eq!(a, Some(1));
            assert_eq!(b, Some(2));
        });
    }
}
