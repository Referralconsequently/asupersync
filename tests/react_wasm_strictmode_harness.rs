#![allow(missing_docs)]

use asupersync::types::{
    ReactProviderConfig, ReactProviderPhase, ReactProviderState, WasmAbiCancellation,
    WasmAbiOutcomeEnvelope, WasmAbiSymbol, WasmAbiValue, WasmBoundaryState, WasmExportDispatcher,
    WasmOutcomeExt, WasmTaskCancelRequest, WasmTaskSpawnBuilder,
};

#[test]
fn strict_mode_double_invocation_is_leak_free_and_cancel_correct() {
    let mut provider = ReactProviderState::new(ReactProviderConfig {
        strict_mode_resilient: true,
        devtools_diagnostics: true,
        ..Default::default()
    });

    let mut expected_cancel_events = 0usize;
    let mut expected_join_events = 0usize;

    for cycle in 0..2 {
        provider.mount().expect("mount should succeed");
        let root_scope = provider
            .root_scope_handle()
            .expect("root scope must exist after mount");
        let child_scope = provider
            .create_child_scope(Some("strict-child"))
            .expect("child scope must be creatable when ready");

        let root_task = provider
            .spawn_task(root_scope, Some("strict-root-task"))
            .expect("root task spawn should succeed");
        let child_task = provider
            .spawn_task(child_scope, Some("strict-child-task"))
            .expect("child task spawn should succeed");

        // Mixed outcomes: one task completes normally before cleanup.
        provider
            .complete_task(
                &root_task,
                WasmAbiOutcomeEnvelope::Ok {
                    value: WasmAbiValue::Unit,
                },
            )
            .expect("task completion should succeed");
        expected_join_events += 1;

        // The remaining task should be cancelled and drained during unmount.
        let _ = child_task;
        expected_cancel_events += 1;
        expected_join_events += 1;

        provider.unmount().expect("unmount should succeed");
        let snapshot = provider.snapshot();
        assert_eq!(snapshot.phase, ReactProviderPhase::Disposed);
        assert_eq!(snapshot.child_scope_count, 0);
        assert_eq!(snapshot.active_task_count, 0);

        let diagnostics = snapshot
            .dispatcher_diagnostics
            .expect("provider snapshot must include diagnostics");
        assert!(
            diagnostics.is_clean(),
            "strict mode cycle {cycle} must be leak-free: {:?}",
            diagnostics.as_log_fields()
        );
    }

    let events = provider.dispatcher().event_log().events();
    let cancel_events = events
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskCancel)
        .collect::<Vec<_>>();
    assert_eq!(
        cancel_events.len(),
        expected_cancel_events,
        "each unmount should emit one cancel for the unfinished task"
    );
    assert!(cancel_events.iter().all(|event| {
        event.state_from == WasmBoundaryState::Active
            && event.state_to == WasmBoundaryState::Cancelling
    }));

    let join_count = events
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskJoin)
        .count();
    assert_eq!(join_count, expected_join_events);
}

#[test]
fn concurrent_render_restart_pattern_cancels_and_drains_losers() {
    let mut dispatcher = WasmExportDispatcher::new();
    let (runtime, scope) = dispatcher
        .create_scoped_runtime(Some("react-concurrent-render"), None)
        .expect("runtime/scope creation should succeed");

    let restart_count = 3usize;
    let mut current = dispatcher
        .spawn(
            WasmTaskSpawnBuilder::new(scope).label("render-attempt-0"),
            None,
        )
        .expect("initial task spawn should succeed");

    for attempt in 1..=restart_count {
        dispatcher
            .task_cancel(
                &WasmTaskCancelRequest {
                    task: current,
                    kind: "dep_change".to_string(),
                    message: Some(format!("restart-{attempt}")),
                },
                None,
            )
            .expect("dep-change cancellation should succeed");

        let cancelled = WasmAbiOutcomeEnvelope::Cancelled {
            cancellation: WasmAbiCancellation {
                kind: "dep_change".to_string(),
                phase: "completed".to_string(),
                origin_region: "react-use-task".to_string(),
                origin_task: None,
                timestamp_nanos: attempt as u64,
                message: Some(format!("restart-{attempt}")),
                truncated: false,
            },
        };

        let loser_outcome = dispatcher
            .task_join(&current, cancelled, None)
            .expect("cancelled task should join cleanly");
        assert!(loser_outcome.is_cancelled());

        current = dispatcher
            .spawn(
                WasmTaskSpawnBuilder::new(scope).label(format!("render-attempt-{attempt}")),
                None,
            )
            .expect("replacement task spawn should succeed");
    }

    let winner_outcome = dispatcher
        .task_join(
            &current,
            WasmAbiOutcomeEnvelope::Ok {
                value: WasmAbiValue::String("winner".to_string()),
            },
            None,
        )
        .expect("winner join should succeed");
    assert!(winner_outcome.is_ok());

    dispatcher
        .close_scoped_runtime(&scope, &runtime, None)
        .expect("structured teardown should succeed");
    let diagnostics = dispatcher.diagnostic_snapshot();
    assert!(
        diagnostics.is_clean(),
        "concurrent restart harness must leave no leaks: {:?}",
        diagnostics.as_log_fields()
    );

    let events = dispatcher.event_log().events();
    let spawn_count = events
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskSpawn)
        .count();
    let cancel_count = events
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskCancel)
        .count();
    let join_count = events
        .iter()
        .filter(|event| event.symbol == WasmAbiSymbol::TaskJoin)
        .count();

    assert_eq!(spawn_count, restart_count + 1);
    assert_eq!(cancel_count, restart_count);
    assert_eq!(join_count, restart_count + 1);

    assert!(
        events
            .iter()
            .filter(|event| event.symbol == WasmAbiSymbol::TaskCancel)
            .all(|event| {
                event.state_from == WasmBoundaryState::Active
                    && event.state_to == WasmBoundaryState::Cancelling
            })
    );
}
