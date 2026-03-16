#![deny(unsafe_code)]

use asupersync::types::{
    ReactProviderConfig, ReactProviderPhase, ReactProviderState, WasmAbiOutcomeEnvelope,
    WasmAbiSymbol, WasmAbiValue,
};
use js_sys::{global, Reflect};
use serde::Serialize;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsValue;
use web_sys::window;

#[derive(Debug, Serialize)]
struct CapabilitySnapshot {
    has_window: bool,
    has_document: bool,
    has_webassembly: bool,
}

#[derive(Debug, Serialize)]
struct DemoSummary {
    scenario_id: &'static str,
    support_lane: &'static str,
    ready_phase: ReactProviderPhase,
    disposed_phase: ReactProviderPhase,
    child_scope_count_before_unmount: usize,
    active_task_count_before_unmount: usize,
    completed_task_outcome: &'static str,
    cancel_event_count: usize,
    dispatch_count: u64,
    event_symbols: Vec<String>,
    diagnostics_clean: bool,
    capabilities: CapabilitySnapshot,
}

fn js_error(message: impl Into<String>) -> JsValue {
    JsValue::from_str(&message.into())
}

fn capability_snapshot() -> CapabilitySnapshot {
    let has_window = window().is_some();
    let has_document = window().and_then(|win| win.document()).is_some();
    let has_webassembly =
        Reflect::has(&global(), &JsValue::from_str("WebAssembly")).unwrap_or(false);

    CapabilitySnapshot {
        has_window,
        has_document,
        has_webassembly,
    }
}

#[wasm_bindgen]
pub fn run_rust_browser_consumer_demo() -> Result<JsValue, JsValue> {
    let mut provider = ReactProviderState::new(ReactProviderConfig {
        label: "rust-browser-consumer".to_string(),
        ..ReactProviderConfig::default()
    });

    provider
        .mount()
        .map_err(|err| js_error(format!("mount failed: {err}")))?;

    let child_scope = provider
        .create_child_scope(Some("rust-browser-child"))
        .map_err(|err| js_error(format!("create_child_scope failed: {err}")))?;

    let completed_task = provider
        .spawn_task(child_scope, Some("completed-task"))
        .map_err(|err| js_error(format!("spawn completed task failed: {err}")))?;

    let completed_outcome = provider
        .complete_task(
            &completed_task,
            WasmAbiOutcomeEnvelope::Ok {
                value: WasmAbiValue::String("rust-browser-consumer-ok".to_string()),
            },
        )
        .map_err(|err| js_error(format!("complete_task failed: {err}")))?;

    provider
        .spawn_task(child_scope, Some("cancel-on-unmount"))
        .map_err(|err| js_error(format!("spawn cancel-on-unmount task failed: {err}")))?;

    let ready_snapshot = provider.snapshot();

    provider
        .unmount()
        .map_err(|err| js_error(format!("unmount failed: {err}")))?;

    let disposed_snapshot = provider.snapshot();
    let event_symbols = provider
        .dispatcher()
        .event_log()
        .events()
        .iter()
        .map(|event| event.symbol.as_str().to_string())
        .collect::<Vec<_>>();
    let cancel_event_count = provider
        .dispatcher()
        .event_log()
        .events()
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskCancel)
        .count();

    let diagnostics = disposed_snapshot
        .dispatcher_diagnostics
        .ok_or_else(|| js_error("dispatcher diagnostics missing after unmount"))?;

    let completed_task_outcome = match completed_outcome {
        WasmAbiOutcomeEnvelope::Ok { .. } => "ok",
        WasmAbiOutcomeEnvelope::Err { .. } => "err",
        WasmAbiOutcomeEnvelope::Cancelled { .. } => "cancelled",
        WasmAbiOutcomeEnvelope::Panicked { .. } => "panicked",
    };

    let summary = DemoSummary {
        scenario_id: "RUST-BROWSER-CONSUMER",
        support_lane: "repository_maintained_rust_browser_fixture",
        ready_phase: ready_snapshot.phase,
        disposed_phase: disposed_snapshot.phase,
        child_scope_count_before_unmount: ready_snapshot.child_scope_count,
        active_task_count_before_unmount: ready_snapshot.active_task_count,
        completed_task_outcome,
        cancel_event_count,
        dispatch_count: diagnostics.dispatch_count,
        event_symbols,
        diagnostics_clean: diagnostics.is_clean(),
        capabilities: capability_snapshot(),
    };

    serde_wasm_bindgen::to_value(&summary)
        .map_err(|err| js_error(format!("failed to encode summary: {err}")))
}
