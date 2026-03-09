//! Tokio runtime context bridge.
//!
//! Provides [`AsupersyncRuntime`], the keystone primitive that implements
//! Tokio's runtime handle interface using Asupersync's executor.
//!
//! This does NOT start a Tokio runtime. It intercepts Tokio runtime
//! operations and routes them to Asupersync equivalents.

use asupersync::Cx;
use asupersync::types::RegionId;

/// A Tokio-compatible runtime handle backed by Asupersync's executor.
///
/// This does NOT start a Tokio runtime. It intercepts Tokio runtime
/// operations and routes them to Asupersync equivalents.
#[derive(Debug, Clone)]
pub struct AsupersyncRuntime {
    cx: Cx,
    region_id: RegionId,
}

impl AsupersyncRuntime {
    /// Create a new AsupersyncRuntime bound to the given context.
    #[must_use]
    pub fn new(cx: &Cx) -> Self {
        Self {
            cx: cx.clone(),
            region_id: cx.region_id(),
        }
    }

    /// Access the underlying Asupersync context captured by this runtime.
    #[must_use]
    pub const fn cx(&self) -> &Cx {
        &self.cx
    }

    /// Return the region that owns tasks spawned through this runtime.
    #[must_use]
    pub const fn region_id(&self) -> RegionId {
        self.region_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asupersync_runtime_creation() {
        let cx = Cx::for_testing();
        let rt = AsupersyncRuntime::new(&cx);
        assert_eq!(rt.region_id(), cx.region_id());
        assert_eq!(rt.cx().region_id(), cx.region_id());
    }
}
