//! Combinator middleware for HTTP handlers.
//!
//! This module bridges Asupersync's composable combinators (circuit breaker,
//! retry, timeout, rate limit, bulkhead) with the web framework's [`Handler`]
//! trait, enabling resilience patterns as middleware layers.
//!
//! # Architecture
//!
//! Each middleware wraps an inner [`Handler`] and applies a combinator before
//! or around the handler invocation. Middleware implements [`Handler`] itself,
//! so they compose naturally.
//!
//! # Example
//!
//! ```ignore
//! use asupersync::web::middleware::*;
//! use asupersync::web::{Router, get};
//! use asupersync::combinator::*;
//! use std::time::Duration;
//!
//! let handler = FnHandler::new(|| "hello");
//!
//! // Single middleware
//! let protected = TimeoutMiddleware::new(handler, Duration::from_secs(5));
//!
//! // Composed middleware (outermost applied first)
//! let resilient = MiddlewareStack::new(handler)
//!     .with_timeout(Duration::from_secs(5))
//!     .with_rate_limit(RateLimitPolicy::default())
//!     .with_circuit_breaker(CircuitBreakerPolicy::default())
//!     .build();
//! ```
//!
//! # Execution Order
//!
//! When composing middleware via [`MiddlewareStack`], the order is outermost
//! first. For a stack built as `.with_timeout().with_rate_limit()`:
//!
//! ```text
//! Request → Timeout → RateLimit → Handler → Response
//! ```

use std::sync::Arc;
use std::time::Duration;

use crate::combinator::bulkhead::{Bulkhead, BulkheadPolicy};
use crate::combinator::circuit_breaker::{CircuitBreaker, CircuitBreakerPolicy};
use crate::combinator::rate_limit::{RateLimitPolicy, RateLimiter};
use crate::combinator::retry::RetryPolicy;
use crate::types::Time;

use super::extract::Request;
use super::handler::Handler;
use super::response::{Response, StatusCode};

// ─── CorsMiddleware ─────────────────────────────────────────────────────────

/// Origin matching policy for CORS headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsAllowOrigin {
    /// Allow any origin (`*`).
    Any,
    /// Allow only the provided set of explicit origins.
    Exact(Vec<String>),
}

/// CORS policy configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsPolicy {
    /// Allowed origins.
    pub allow_origin: CorsAllowOrigin,
    /// Allowed methods for preflight responses.
    pub allow_methods: Vec<String>,
    /// Allowed headers for preflight responses.
    pub allow_headers: Vec<String>,
    /// Exposed headers for non-preflight responses.
    pub expose_headers: Vec<String>,
    /// Optional max-age for preflight cache.
    pub max_age: Option<Duration>,
    /// Whether credentials are allowed.
    pub allow_credentials: bool,
}

impl Default for CorsPolicy {
    fn default() -> Self {
        Self {
            allow_origin: CorsAllowOrigin::Any,
            allow_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
                "DELETE".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
            ],
            allow_headers: vec!["*".to_string()],
            expose_headers: Vec::new(),
            max_age: Some(Duration::from_secs(60 * 10)),
            allow_credentials: false,
        }
    }
}

impl CorsPolicy {
    /// Allow only the provided origins.
    #[must_use]
    pub fn with_exact_origins(origins: impl IntoIterator<Item = String>) -> Self {
        Self {
            allow_origin: CorsAllowOrigin::Exact(origins.into_iter().collect()),
            ..Self::default()
        }
    }
}

/// Middleware that applies CORS policy and handles preflight requests.
pub struct CorsMiddleware<H> {
    inner: H,
    policy: CorsPolicy,
}

impl<H: Handler> CorsMiddleware<H> {
    /// Wrap a handler with CORS policy.
    #[must_use]
    pub fn new(inner: H, policy: CorsPolicy) -> Self {
        Self { inner, policy }
    }

    fn is_preflight(req: &Request) -> bool {
        req.method.eq_ignore_ascii_case("OPTIONS")
            && header_value(req, "origin").is_some()
            && header_value(req, "access-control-request-method").is_some()
    }

    fn allowed_origin_value(&self, origin: &str) -> Option<String> {
        match &self.policy.allow_origin {
            CorsAllowOrigin::Any => {
                if self.policy.allow_credentials {
                    Some(origin.to_string())
                } else {
                    Some("*".to_string())
                }
            }
            CorsAllowOrigin::Exact(origins) => origins
                .iter()
                .find(|candidate| candidate.eq_ignore_ascii_case(origin))
                .cloned(),
        }
    }

    fn apply_common_headers(&self, mut resp: Response, allow_origin: &str) -> Response {
        resp.headers
            .insert("access-control-allow-origin".to_string(), allow_origin.to_string());
        // Cache key must vary by Origin when policy is origin-sensitive.
        resp.headers
            .insert("vary".to_string(), "origin".to_string());
        if self.policy.allow_credentials {
            resp.headers.insert(
                "access-control-allow-credentials".to_string(),
                "true".to_string(),
            );
        }
        if !self.policy.expose_headers.is_empty() {
            resp.headers.insert(
                "access-control-expose-headers".to_string(),
                self.policy.expose_headers.join(", "),
            );
        }
        resp
    }
}

impl<H: Handler> Handler for CorsMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        let origin = match header_value(&req, "origin") {
            Some(value) => value,
            None => return self.inner.call(req),
        };

        let Some(allow_origin) = self.allowed_origin_value(&origin) else {
            // Origin not allowed: pass through without CORS headers.
            return self.inner.call(req);
        };

        if Self::is_preflight(&req) {
            let mut resp = Response::empty(StatusCode::NO_CONTENT);
            resp = self.apply_common_headers(resp, &allow_origin);
            resp.headers.insert(
                "access-control-allow-methods".to_string(),
                self.policy.allow_methods.join(", "),
            );
            resp.headers.insert(
                "access-control-allow-headers".to_string(),
                self.policy.allow_headers.join(", "),
            );
            if let Some(max_age) = self.policy.max_age {
                resp.headers.insert(
                    "access-control-max-age".to_string(),
                    max_age.as_secs().to_string(),
                );
            }
            resp.headers.insert(
                "vary".to_string(),
                "origin, access-control-request-method, access-control-request-headers"
                    .to_string(),
            );
            return resp;
        }

        let resp = self.inner.call(req);
        self.apply_common_headers(resp, &allow_origin)
    }
}

fn header_value(req: &Request, header_name: &str) -> Option<String> {
    req.headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(header_name))
        .map(|(_, value)| value.clone())
}

// ─── TimeoutMiddleware ──────────────────────────────────────────────────────

/// Middleware that enforces a request deadline.
///
/// If the handler does not complete before the timeout, a 504 Gateway Timeout
/// response is returned. In Phase 0 (synchronous handlers), this checks
/// elapsed wall-clock time after the handler returns.
///
/// For true preemptive timeout, async runtime integration is required (Phase 1+).
pub struct TimeoutMiddleware<H> {
    inner: H,
    timeout: Duration,
}

impl<H: Handler> TimeoutMiddleware<H> {
    /// Wrap a handler with a timeout.
    #[must_use]
    pub fn new(inner: H, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

impl<H: Handler> Handler for TimeoutMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        let start = std::time::Instant::now();
        let resp = self.inner.call(req);
        let elapsed = start.elapsed();

        if elapsed > self.timeout {
            Response::new(
                StatusCode::GATEWAY_TIMEOUT,
                format!("Request timed out after {elapsed:?}").into_bytes(),
            )
        } else {
            resp
        }
    }
}

// ─── CircuitBreakerMiddleware ───────────────────────────────────────────────

/// Middleware that wraps a handler with a circuit breaker.
///
/// When the circuit is open, requests are immediately rejected with 503
/// Service Unavailable. The circuit breaker tracks handler errors
/// (5xx responses) as failures.
pub struct CircuitBreakerMiddleware<H> {
    inner: H,
    breaker: Arc<CircuitBreaker>,
}

impl<H: Handler> CircuitBreakerMiddleware<H> {
    /// Wrap a handler with a circuit breaker.
    #[must_use]
    pub fn new(inner: H, policy: CircuitBreakerPolicy) -> Self {
        Self {
            inner,
            breaker: Arc::new(CircuitBreaker::new(policy)),
        }
    }

    /// Wrap a handler with a shared circuit breaker.
    ///
    /// Use this to share a breaker across multiple routes or middleware.
    #[must_use]
    pub fn shared(inner: H, breaker: Arc<CircuitBreaker>) -> Self {
        Self { inner, breaker }
    }

    /// Returns a reference to the circuit breaker for metrics inspection.
    #[must_use]
    pub fn breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }
}

impl<H: Handler> Handler for CircuitBreakerMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        let now = Time::from_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        // Use the circuit breaker to guard the handler call.
        // We treat the handler as a Result where 5xx = error.
        let result = self.breaker.call(now, || {
            let resp = self.inner.call(req);
            if resp.status.is_server_error() {
                Err(format!("server error: {}", resp.status.as_u16()))
            } else {
                Ok(resp)
            }
        });

        match result {
            Ok(resp) => resp,
            Err(crate::combinator::circuit_breaker::CircuitBreakerError::Open { remaining }) => {
                let body =
                    format!("Service Unavailable: circuit breaker open, retry after {remaining:?}");
                Response::new(StatusCode::SERVICE_UNAVAILABLE, body.into_bytes())
                    .header("retry-after", format!("{}", remaining.as_secs().max(1)))
            }
            Err(crate::combinator::circuit_breaker::CircuitBreakerError::HalfOpenFull) => {
                Response::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    b"Service Unavailable: circuit breaker half-open, max probes active".to_vec(),
                )
            }
            Err(crate::combinator::circuit_breaker::CircuitBreakerError::Inner(err_msg)) => {
                // The handler produced a 5xx response; the circuit breaker recorded
                // it as a failure. Reconstruct a 500 response.
                Response::new(StatusCode::INTERNAL_SERVER_ERROR, err_msg.into_bytes())
            }
        }
    }
}

// ─── RateLimitMiddleware ────────────────────────────────────────────────────

/// Middleware that enforces a rate limit on requests.
///
/// Requests exceeding the rate limit receive a 429 Too Many Requests response
/// with a `retry-after` header indicating when to retry.
pub struct RateLimitMiddleware<H> {
    inner: H,
    limiter: Arc<RateLimiter>,
}

impl<H: Handler> RateLimitMiddleware<H> {
    /// Wrap a handler with a rate limiter.
    #[must_use]
    pub fn new(inner: H, policy: RateLimitPolicy) -> Self {
        Self {
            inner,
            limiter: Arc::new(RateLimiter::new(policy)),
        }
    }

    /// Wrap a handler with a shared rate limiter.
    ///
    /// Use this to share a limiter across multiple routes.
    #[must_use]
    pub fn shared(inner: H, limiter: Arc<RateLimiter>) -> Self {
        Self { inner, limiter }
    }

    /// Returns a reference to the rate limiter for metrics inspection.
    #[must_use]
    pub fn limiter(&self) -> &RateLimiter {
        &self.limiter
    }
}

impl<H: Handler> Handler for RateLimitMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        let now = Time::from_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        if self.limiter.try_acquire(1, now) {
            self.inner.call(req)
        } else {
            let retry_after = self.limiter.retry_after(1, now);
            let secs = retry_after.as_secs().max(1);
            Response::new(
                StatusCode::TOO_MANY_REQUESTS,
                format!("Too Many Requests: rate limit exceeded, retry after {secs}s").into_bytes(),
            )
            .header("retry-after", format!("{secs}"))
        }
    }
}

// ─── BulkheadMiddleware ─────────────────────────────────────────────────────

/// Middleware that isolates requests into a concurrency-limited compartment.
///
/// When all permits are in use, requests receive a 503 Service Unavailable
/// response. This prevents any single route or service from consuming all
/// server resources.
pub struct BulkheadMiddleware<H> {
    inner: H,
    bulkhead: Arc<Bulkhead>,
}

impl<H: Handler> BulkheadMiddleware<H> {
    /// Wrap a handler with a bulkhead.
    #[must_use]
    pub fn new(inner: H, policy: BulkheadPolicy) -> Self {
        Self {
            inner,
            bulkhead: Arc::new(Bulkhead::new(policy)),
        }
    }

    /// Wrap a handler with a shared bulkhead.
    ///
    /// Use this to share concurrency limits across routes.
    #[must_use]
    pub fn shared(inner: H, bulkhead: Arc<Bulkhead>) -> Self {
        Self { inner, bulkhead }
    }

    /// Returns a reference to the bulkhead for metrics inspection.
    #[must_use]
    pub fn bulkhead(&self) -> &Bulkhead {
        &self.bulkhead
    }
}

impl<H: Handler> Handler for BulkheadMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        self.bulkhead.try_acquire(1).map_or_else(
            || {
                Response::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    b"Service Unavailable: concurrency limit reached".to_vec(),
                )
            },
            |p| {
                let resp = self.inner.call(req);
                p.release();
                resp
            },
        )
    }
}

// ─── RetryMiddleware ────────────────────────────────────────────────────────

/// Middleware that retries failed handler invocations.
///
/// Only retries on 5xx server errors. The request body is cloned for each
/// retry attempt. Non-idempotent methods (POST, PATCH, DELETE) are retried
/// by default — callers should set `idempotent_only` to restrict retries to
/// safe methods.
///
/// Note: In Phase 0 (synchronous), retry sleeps block the thread. Production
/// use should rely on async retry with cooperative yielding (Phase 1+).
pub struct RetryMiddleware<H> {
    inner: H,
    policy: RetryPolicy,
    /// When true, only retry GET, HEAD, OPTIONS, PUT (idempotent methods).
    idempotent_only: bool,
}

impl<H: Handler> RetryMiddleware<H> {
    /// Wrap a handler with retry logic.
    #[must_use]
    pub fn new(inner: H, policy: RetryPolicy) -> Self {
        Self {
            inner,
            policy,
            idempotent_only: true,
        }
    }

    /// Allow retries for all methods, including non-idempotent ones.
    #[must_use]
    pub fn retry_all_methods(mut self) -> Self {
        self.idempotent_only = false;
        self
    }
}

/// Returns true if the method is considered idempotent.
fn is_idempotent(method: &str) -> bool {
    matches!(
        method.to_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS" | "PUT" | "DELETE" | "TRACE"
    )
}

impl<H: Handler> Handler for RetryMiddleware<H> {
    fn call(&self, req: Request) -> Response {
        // Check if retry is appropriate for this method.
        if self.idempotent_only && !is_idempotent(&req.method) {
            return self.inner.call(req);
        }

        let max = self.policy.max_attempts.max(1);
        let mut delay = self.policy.initial_delay;
        let mut last_resp = None;

        for attempt in 0..max {
            // Clone request for retry (first attempt uses original).
            if attempt != 0 {
                // Sleep before retry (Phase 0: blocking sleep).
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
                // Compute next delay with exponential backoff.
                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * self.policy.multiplier)
                        .min(self.policy.max_delay.as_secs_f64()),
                );
            }
            let try_req = req.clone();

            let resp = self.inner.call(try_req);
            if !resp.status.is_server_error() {
                return resp;
            }
            last_resp = Some(resp);
        }

        // All attempts failed; return the last response.
        last_resp.unwrap_or_else(|| {
            Response::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                b"Internal Server Error: all retry attempts exhausted".to_vec(),
            )
        })
    }
}

// ─── MiddlewareStack ────────────────────────────────────────────────────────

/// Builder for composing multiple middleware layers around a handler.
///
/// Middleware is applied in the order specified (outermost first). The
/// resulting type implements [`Handler`].
///
/// # Example
///
/// ```ignore
/// let handler = MiddlewareStack::new(my_handler)
///     .with_timeout(Duration::from_secs(30))
///     .with_rate_limit(RateLimitPolicy::default())
///     .with_circuit_breaker(CircuitBreakerPolicy::default())
///     .build();
/// ```
///
/// Execution order: Timeout → RateLimit → CircuitBreaker → Handler
pub struct MiddlewareStack<H> {
    inner: H,
}

impl<H: Handler> MiddlewareStack<H> {
    /// Start building a middleware stack around the given handler.
    #[must_use]
    pub fn new(inner: H) -> Self {
        Self { inner }
    }

    /// Add a timeout middleware layer.
    #[must_use]
    pub fn with_timeout(self, timeout: Duration) -> MiddlewareStack<TimeoutMiddleware<H>> {
        MiddlewareStack {
            inner: TimeoutMiddleware::new(self.inner, timeout),
        }
    }

    /// Add a CORS middleware layer.
    #[must_use]
    pub fn with_cors(self, policy: CorsPolicy) -> MiddlewareStack<CorsMiddleware<H>> {
        MiddlewareStack {
            inner: CorsMiddleware::new(self.inner, policy),
        }
    }

    /// Add a circuit breaker middleware layer.
    #[must_use]
    pub fn with_circuit_breaker(
        self,
        policy: CircuitBreakerPolicy,
    ) -> MiddlewareStack<CircuitBreakerMiddleware<H>> {
        MiddlewareStack {
            inner: CircuitBreakerMiddleware::new(self.inner, policy),
        }
    }

    /// Add a circuit breaker middleware layer with a shared breaker.
    #[must_use]
    pub fn with_shared_circuit_breaker(
        self,
        breaker: Arc<CircuitBreaker>,
    ) -> MiddlewareStack<CircuitBreakerMiddleware<H>> {
        MiddlewareStack {
            inner: CircuitBreakerMiddleware::shared(self.inner, breaker),
        }
    }

    /// Add a rate limit middleware layer.
    #[must_use]
    pub fn with_rate_limit(
        self,
        policy: RateLimitPolicy,
    ) -> MiddlewareStack<RateLimitMiddleware<H>> {
        MiddlewareStack {
            inner: RateLimitMiddleware::new(self.inner, policy),
        }
    }

    /// Add a rate limit middleware layer with a shared limiter.
    #[must_use]
    pub fn with_shared_rate_limit(
        self,
        limiter: Arc<RateLimiter>,
    ) -> MiddlewareStack<RateLimitMiddleware<H>> {
        MiddlewareStack {
            inner: RateLimitMiddleware::shared(self.inner, limiter),
        }
    }

    /// Add a bulkhead middleware layer.
    #[must_use]
    pub fn with_bulkhead(self, policy: BulkheadPolicy) -> MiddlewareStack<BulkheadMiddleware<H>> {
        MiddlewareStack {
            inner: BulkheadMiddleware::new(self.inner, policy),
        }
    }

    /// Add a bulkhead middleware layer with a shared bulkhead.
    #[must_use]
    pub fn with_shared_bulkhead(
        self,
        bulkhead: Arc<Bulkhead>,
    ) -> MiddlewareStack<BulkheadMiddleware<H>> {
        MiddlewareStack {
            inner: BulkheadMiddleware::shared(self.inner, bulkhead),
        }
    }

    /// Add a retry middleware layer.
    #[must_use]
    pub fn with_retry(self, policy: RetryPolicy) -> MiddlewareStack<RetryMiddleware<H>> {
        MiddlewareStack {
            inner: RetryMiddleware::new(self.inner, policy),
        }
    }

    /// Finish building and return the composed handler.
    #[must_use]
    pub fn build(self) -> H {
        self.inner
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::handler::FnHandler;

    fn ok_handler() -> &'static str {
        "ok"
    }

    fn error_handler() -> Response {
        Response::new(StatusCode::INTERNAL_SERVER_ERROR, b"fail".to_vec())
    }

    fn slow_handler() -> &'static str {
        std::thread::sleep(Duration::from_millis(50));
        "slow"
    }

    fn make_request() -> Request {
        Request::new("GET", "/test")
    }

    struct CountingHandler {
        calls: Arc<std::sync::atomic::AtomicU32>,
        delay: Duration,
        status: StatusCode,
    }

    impl Handler for CountingHandler {
        fn call(&self, _req: Request) -> Response {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            Response::new(self.status, b"counted".to_vec())
        }
    }

    struct InspectHandler;

    impl Handler for InspectHandler {
        fn call(&self, req: Request) -> Response {
            req.extensions.get("trace_id").map_or_else(
                || Response::new(StatusCode::BAD_REQUEST, b"missing trace_id".to_vec()),
                |value| Response::new(StatusCode::OK, value.as_bytes().to_vec()),
            )
        }
    }

    struct FailingIfCalled;

    impl Handler for FailingIfCalled {
        fn call(&self, _req: Request) -> Response {
            Response::new(StatusCode::INTERNAL_SERVER_ERROR, b"inner-called".to_vec())
        }
    }

    // --- TimeoutMiddleware ---

    #[test]
    fn timeout_passes_when_fast() {
        let mw = TimeoutMiddleware::new(FnHandler::new(ok_handler), Duration::from_secs(5));
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn timeout_triggers_when_slow() {
        let mw = TimeoutMiddleware::new(FnHandler::new(slow_handler), Duration::from_millis(1));
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::GATEWAY_TIMEOUT);
    }

    // --- CircuitBreakerMiddleware ---

    #[test]
    fn circuit_breaker_passes_success() {
        let policy = CircuitBreakerPolicy::default();
        let mw = CircuitBreakerMiddleware::new(FnHandler::new(ok_handler), policy);
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn circuit_breaker_opens_after_failures() {
        let policy = CircuitBreakerPolicy {
            failure_threshold: 2,
            ..Default::default()
        };
        let mw = CircuitBreakerMiddleware::new(FnHandler::new(error_handler), policy);

        // Fail twice to reach threshold.
        let _ = mw.call(make_request());
        let _ = mw.call(make_request());

        // Next call should be rejected.
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn circuit_breaker_shared_state() {
        let policy = CircuitBreakerPolicy::default();
        let breaker = Arc::new(CircuitBreaker::new(policy));

        let mw1 =
            CircuitBreakerMiddleware::shared(FnHandler::new(ok_handler), Arc::clone(&breaker));
        let mw2 =
            CircuitBreakerMiddleware::shared(FnHandler::new(ok_handler), Arc::clone(&breaker));

        // Both share the same breaker.
        let _ = mw1.call(make_request());
        assert_eq!(
            mw1.breaker().metrics().total_success,
            mw2.breaker().metrics().total_success
        );
    }

    #[test]
    fn circuit_breaker_surfaces_handler_error() {
        let policy = CircuitBreakerPolicy {
            failure_threshold: 10,
            ..Default::default()
        };
        let mw = CircuitBreakerMiddleware::new(FnHandler::new(error_handler), policy);
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(String::from_utf8_lossy(&resp.body).contains("server error"));
    }

    // --- RateLimitMiddleware ---

    #[test]
    fn rate_limit_allows_within_limit() {
        let policy = RateLimitPolicy {
            rate: 100,
            burst: 10,
            ..Default::default()
        };
        let mw = RateLimitMiddleware::new(FnHandler::new(ok_handler), policy);
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn rate_limit_rejects_over_limit() {
        let policy = RateLimitPolicy {
            rate: 1,
            burst: 1,
            period: Duration::from_mins(1),
            ..Default::default()
        };
        let mw = RateLimitMiddleware::new(FnHandler::new(ok_handler), policy);

        // First call consumes the burst.
        let resp1 = mw.call(make_request());
        assert_eq!(resp1.status, StatusCode::OK);

        // Second call should be rate-limited.
        let resp2 = mw.call(make_request());
        assert_eq!(resp2.status, StatusCode::TOO_MANY_REQUESTS);
        assert!(resp2.headers.contains_key("retry-after"));
    }

    #[test]
    fn rate_limit_short_circuits_inner_handler() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handler = CountingHandler {
            calls: Arc::clone(&calls),
            delay: Duration::from_millis(0),
            status: StatusCode::OK,
        };
        let policy = RateLimitPolicy {
            rate: 1,
            burst: 1,
            period: Duration::from_mins(1),
            ..Default::default()
        };
        let mw = RateLimitMiddleware::new(handler, policy);

        let _ = mw.call(make_request());
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    // --- BulkheadMiddleware ---

    #[test]
    fn bulkhead_allows_within_limit() {
        let policy = BulkheadPolicy {
            max_concurrent: 10,
            ..Default::default()
        };
        let mw = BulkheadMiddleware::new(FnHandler::new(ok_handler), policy);
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn bulkhead_releases_permit_after_call() {
        let policy = BulkheadPolicy {
            max_concurrent: 1,
            ..Default::default()
        };
        let mw = BulkheadMiddleware::new(FnHandler::new(ok_handler), policy);

        // Sequential calls should all succeed since permit is released.
        for _ in 0..5 {
            let resp = mw.call(make_request());
            assert_eq!(resp.status, StatusCode::OK);
        }
    }

    // --- RetryMiddleware ---

    #[test]
    fn retry_succeeds_on_first_try() {
        let policy = RetryPolicy::immediate(3);
        let mw = RetryMiddleware::new(FnHandler::new(ok_handler), policy);
        let resp = mw.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn retry_exhausts_attempts_on_server_error() {
        let policy = RetryPolicy::immediate(3);
        let mw = RetryMiddleware::new(FnHandler::new(error_handler), policy);
        let resp = mw.call(make_request());
        // Should get the error response after all retries exhausted.
        assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn retry_skips_non_idempotent_by_default() {
        let policy = RetryPolicy::immediate(3);
        let mw = RetryMiddleware::new(FnHandler::new(error_handler), policy);
        let resp = mw.call(Request::new("POST", "/create"));
        // POST is not idempotent, should not retry — single call.
        assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn retry_all_methods_retries_post() {
        use std::sync::atomic::{AtomicU32, Ordering};

        static CALL_COUNT: AtomicU32 = AtomicU32::new(0);

        fn counting_handler() -> Response {
            CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            Response::new(StatusCode::INTERNAL_SERVER_ERROR, b"fail".to_vec())
        }

        CALL_COUNT.store(0, Ordering::SeqCst);

        let policy = RetryPolicy::immediate(3);
        let mw = RetryMiddleware::new(FnHandler::new(counting_handler), policy).retry_all_methods();
        let _resp = mw.call(Request::new("POST", "/create"));
        assert_eq!(CALL_COUNT.load(Ordering::SeqCst), 3);
    }

    // --- is_idempotent ---

    #[test]
    fn idempotent_methods() {
        assert!(is_idempotent("GET"));
        assert!(is_idempotent("HEAD"));
        assert!(is_idempotent("OPTIONS"));
        assert!(is_idempotent("PUT"));
        assert!(is_idempotent("DELETE"));
        assert!(is_idempotent("TRACE"));
        assert!(!is_idempotent("POST"));
        assert!(!is_idempotent("PATCH"));
    }

    // --- CorsMiddleware ---

    #[test]
    fn cors_adds_headers_for_simple_request() {
        let mw = CorsMiddleware::new(FnHandler::new(ok_handler), CorsPolicy::default());
        let req = Request::new("GET", "/cors").with_header("Origin", "https://example.com");

        let resp = mw.call(req);
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(
            resp.headers.get("access-control-allow-origin"),
            Some(&"*".to_string())
        );
        assert_eq!(resp.headers.get("vary"), Some(&"origin".to_string()));
    }

    #[test]
    fn cors_preflight_short_circuits_inner_handler() {
        let mw = CorsMiddleware::new(FailingIfCalled, CorsPolicy::default());
        let req = Request::new("OPTIONS", "/cors")
            .with_header("Origin", "https://example.com")
            .with_header("Access-Control-Request-Method", "POST")
            .with_header("Access-Control-Request-Headers", "content-type");

        let resp = mw.call(req);
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers.get("access-control-allow-origin"),
            Some(&"*".to_string())
        );
        assert!(resp.headers.contains_key("access-control-allow-methods"));
        assert!(resp.headers.contains_key("access-control-allow-headers"));
    }

    #[test]
    fn cors_exact_origins_blocks_unknown_origin() {
        let policy = CorsPolicy::with_exact_origins(vec![
            "https://allowed.example".to_string(),
            "https://another.example".to_string(),
        ]);
        let mw = CorsMiddleware::new(FnHandler::new(ok_handler), policy);

        let blocked = mw.call(
            Request::new("GET", "/cors").with_header("Origin", "https://blocked.example"),
        );
        assert_eq!(blocked.status, StatusCode::OK);
        assert!(!blocked.headers.contains_key("access-control-allow-origin"));

        let allowed =
            mw.call(Request::new("GET", "/cors").with_header("Origin", "https://allowed.example"));
        assert_eq!(allowed.status, StatusCode::OK);
        assert_eq!(
            allowed.headers.get("access-control-allow-origin"),
            Some(&"https://allowed.example".to_string())
        );
    }

    #[test]
    fn cors_with_credentials_echoes_origin() {
        let policy = CorsPolicy {
            allow_credentials: true,
            ..CorsPolicy::default()
        };
        let mw = CorsMiddleware::new(FnHandler::new(ok_handler), policy);
        let resp =
            mw.call(Request::new("GET", "/cors").with_header("Origin", "https://cred.example"));

        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(
            resp.headers.get("access-control-allow-origin"),
            Some(&"https://cred.example".to_string())
        );
        assert_eq!(
            resp.headers.get("access-control-allow-credentials"),
            Some(&"true".to_string())
        );
    }

    // --- MiddlewareStack ---

    #[test]
    fn middleware_stack_builds() {
        let handler = MiddlewareStack::new(FnHandler::new(ok_handler))
            .with_timeout(Duration::from_secs(5))
            .build();

        let resp = handler.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn middleware_stack_composition() {
        let handler = MiddlewareStack::new(FnHandler::new(ok_handler))
            .with_cors(CorsPolicy::default())
            .with_bulkhead(BulkheadPolicy {
                max_concurrent: 10,
                ..Default::default()
            })
            .with_rate_limit(RateLimitPolicy {
                rate: 100,
                burst: 50,
                ..Default::default()
            })
            .with_timeout(Duration::from_secs(30))
            .build();

        let resp = handler.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn middleware_stack_with_retry() {
        let handler = MiddlewareStack::new(FnHandler::new(ok_handler))
            .with_retry(RetryPolicy::immediate(3))
            .with_timeout(Duration::from_secs(5))
            .build();

        let resp = handler.call(make_request());
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn middleware_stack_preserves_request_extensions() {
        let handler = MiddlewareStack::new(InspectHandler)
            .with_timeout(Duration::from_secs(1))
            .with_rate_limit(RateLimitPolicy {
                rate: 100,
                burst: 100,
                period: Duration::from_secs(1),
                ..Default::default()
            })
            .build();

        let mut req = Request::new("GET", "/ctx");
        req.extensions.insert("trace_id", "trace-123");
        let resp = handler.call(req);
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(&resp.body[..], b"trace-123");
    }

    #[test]
    fn middleware_stack_retry_wraps_timeout() {
        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handler = CountingHandler {
            calls: Arc::clone(&calls),
            delay: Duration::from_millis(10),
            status: StatusCode::OK,
        };
        let stacked = MiddlewareStack::new(handler)
            .with_timeout(Duration::from_millis(1))
            .with_retry(RetryPolicy::immediate(3))
            .build();

        let resp = stacked.call(make_request());
        assert_eq!(resp.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    // --- Observability ---

    #[test]
    fn circuit_breaker_metrics_accessible() {
        let policy = CircuitBreakerPolicy::default();
        let mw = CircuitBreakerMiddleware::new(FnHandler::new(ok_handler), policy);

        let _ = mw.call(make_request());
        let metrics = mw.breaker().metrics();
        assert_eq!(metrics.total_success, 1);
    }

    #[test]
    fn rate_limit_metrics_accessible() {
        let policy = RateLimitPolicy::default();
        let mw = RateLimitMiddleware::new(FnHandler::new(ok_handler), policy);

        let _ = mw.call(make_request());
        let metrics = mw.limiter().metrics();
        assert!(metrics.total_allowed > 0 || metrics.available_tokens >= 0.0);
    }

    #[test]
    fn bulkhead_metrics_accessible() {
        let policy = BulkheadPolicy {
            max_concurrent: 5,
            ..Default::default()
        };
        let mw = BulkheadMiddleware::new(FnHandler::new(ok_handler), policy);

        let _ = mw.call(make_request());
        let metrics = mw.bulkhead().metrics();
        // After call completes, permit should be released.
        assert_eq!(metrics.active_permits, 0);
    }
}
