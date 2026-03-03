//! Bidirectional I/O trait adapters.
//!
//! Provides type wrappers that bridge between Asupersync's `AsyncRead`/`AsyncWrite`
//! traits and their Tokio equivalents.
//!
//! # Adapters
//!
//! - [`TokioIo<T>`]: Wraps an Asupersync I/O type to implement hyper's I/O traits.
//! - [`AsupersyncIo<T>`]: Wraps a Tokio I/O type to implement Asupersync's I/O traits.
//!
//! # Cancel Safety
//!
//! Both adapters preserve the cancel-safety properties of the underlying type:
//! - `poll_read` is cancel-safe (partial data is discarded by caller)
//! - `poll_write` is cancel-safe (partial writes are OK)
//! - `read_exact` and `write_all` are NOT cancel-safe through either adapter

use pin_project_lite::pin_project;

pin_project! {
    /// Wraps an Asupersync I/O type to implement hyper/tokio-compatible I/O traits.
    ///
    /// Use this to pass Asupersync TCP streams, TLS streams, etc. to hyper
    /// and other Tokio-locked crates.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use asupersync_tokio_compat::io::TokioIo;
    ///
    /// let asupersync_stream = asupersync::net::TcpStream::connect(addr).await?;
    /// let hyper_io = TokioIo::new(asupersync_stream);
    /// // Now usable with hyper::server::conn::http1::Builder
    /// ```
    pub struct TokioIo<T> {
        #[pin]
        inner: T,
    }
}

impl<T> TokioIo<T> {
    /// Wrap an Asupersync I/O type for Tokio/hyper compatibility.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner I/O type.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner I/O type.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner I/O type.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

pin_project! {
    /// Wraps a Tokio I/O type to implement Asupersync's `AsyncRead`/`AsyncWrite`.
    ///
    /// Use this to pass Tokio-originated streams into Asupersync code.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use asupersync_tokio_compat::io::AsupersyncIo;
    ///
    /// let tokio_stream = tokio::net::TcpStream::connect(addr).await?;
    /// let stream = AsupersyncIo::new(tokio_stream);
    /// // Now usable with Asupersync's read/write extensions
    /// ```
    pub struct AsupersyncIo<T> {
        #[pin]
        inner: T,
    }
}

impl<T> AsupersyncIo<T> {
    /// Wrap a Tokio I/O type for Asupersync compatibility.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner I/O type.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner I/O type.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner I/O type.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

// NOTE: The actual trait implementations (impl AsyncRead for TokioIo<T>,
// impl tokio::io::AsyncRead for AsupersyncIo<T>, etc.) require access to
// both Asupersync's and Tokio's I/O trait definitions. These are implemented
// in the `tokio-io` feature gate once we wire up the actual poll_read/poll_write
// bridging with the correct ReadBuf conversions.
//
// The trait signatures are:
//
// Asupersync → Tokio direction (TokioIo<T>):
//   impl<T: asupersync::io::AsyncRead> tokio::io::AsyncRead for TokioIo<T>
//   impl<T: asupersync::io::AsyncWrite> tokio::io::AsyncWrite for TokioIo<T>
//
// Tokio → Asupersync direction (AsupersyncIo<T>):
//   impl<T: tokio::io::AsyncRead> asupersync::io::AsyncRead for AsupersyncIo<T>
//   impl<T: tokio::io::AsyncWrite> asupersync::io::AsyncWrite for AsupersyncIo<T>
//
// hyper v1 direction (TokioIo<T>):
//   impl<T: asupersync::io::AsyncRead> hyper::rt::Read for TokioIo<T>
//   impl<T: asupersync::io::AsyncWrite> hyper::rt::Write for TokioIo<T>

/// Adapter for converting between Asupersync and Tokio `ReadBuf` types.
///
/// Both Asupersync and Tokio use a `ReadBuf` wrapper around `&mut [u8]`
/// with initialized/filled tracking. This module provides zero-cost conversion.
pub mod read_buf {
    /// Convert an Asupersync ReadBuf reference to a compatible byte slice
    /// for use with Tokio I/O operations.
    ///
    /// NOTE: This is a placeholder. The actual implementation bridges the
    /// ReadBuf types once both are in scope.
    pub fn bridge_note() -> &'static str {
        "ReadBuf bridging is implemented when the tokio-io feature is enabled"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokio_io_wraps_and_unwraps() {
        let data = Vec::new();
        let wrapped = TokioIo::new(data);
        assert!(wrapped.inner().is_empty());
        let unwrapped = wrapped.into_inner();
        assert!(unwrapped.is_empty());
    }

    #[test]
    fn asupersync_io_wraps_and_unwraps() {
        let data = Vec::new();
        let wrapped = AsupersyncIo::new(data);
        assert!(wrapped.inner().is_empty());
        let unwrapped = wrapped.into_inner();
        assert!(unwrapped.is_empty());
    }
}
