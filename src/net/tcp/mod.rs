pub mod listener;
pub mod socket;
pub mod split;
pub mod stream;
pub mod traits;
pub mod virtual_tcp;

#[cfg(target_arch = "wasm32")]
pub(crate) fn browser_tcp_unsupported(op: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "{op} is unavailable in wasm-browser profiles; use browser transport bindings or VirtualTcp"
        ),
    )
}

// Re-export trait types for convenience
pub use stream::TcpStreamBuilder;
pub use traits::{
    IncomingStream, TcpListenerApi, TcpListenerBuilder, TcpListenerExt, TcpStreamApi,
};
pub use virtual_tcp::{VirtualConnectionInjector, VirtualTcpListener, VirtualTcpStream};
