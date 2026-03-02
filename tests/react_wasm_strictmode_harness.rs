#![allow(missing_docs)]

use asupersync::types::{
    ReactProviderConfig, ReactProviderPhase, ReactProviderState, WasmAbiCancellation,
    WasmAbiOutcomeEnvelope, WasmAbiSymbol, WasmAbiValue, WasmBoundaryState, WasmExportDispatcher,
    WasmHandleRef, WasmOutcomeExt, WasmTaskCancelRequest, WasmTaskSpawnBuilder,
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

#[test]
fn rapid_restart_churn_keeps_event_sequence_balanced() {
    let mut dispatcher = WasmExportDispatcher::new();
    let (runtime, scope) = dispatcher
        .create_scoped_runtime(Some("react-restart-churn"), None)
        .expect("runtime/scope creation should succeed");

    let restart_count = 12usize;
    let mut current = dispatcher
        .spawn(
            WasmTaskSpawnBuilder::new(scope).label("restart-churn-attempt-0"),
            None,
        )
        .expect("initial task spawn should succeed");

    for attempt in 1..=restart_count {
        dispatcher
            .task_cancel(
                &WasmTaskCancelRequest {
                    task: current,
                    kind: "render_churn".to_string(),
                    message: Some(format!("restart-churn-{attempt}")),
                },
                None,
            )
            .expect("restart-churn cancellation should succeed");

        let cancelled = WasmAbiOutcomeEnvelope::Cancelled {
            cancellation: WasmAbiCancellation {
                kind: "render_churn".to_string(),
                phase: "completed".to_string(),
                origin_region: "react-use-task".to_string(),
                origin_task: None,
                timestamp_nanos: attempt as u64,
                message: Some(format!("restart-churn-{attempt}")),
                truncated: false,
            },
        };

        let outcome = dispatcher
            .task_join(&current, cancelled, None)
            .expect("cancelled restart-churn task should join cleanly");
        assert!(outcome.is_cancelled());

        current = dispatcher
            .spawn(
                WasmTaskSpawnBuilder::new(scope).label(format!("restart-churn-attempt-{attempt}")),
                None,
            )
            .expect("replacement task spawn should succeed");
    }

    let winner = dispatcher
        .task_join(
            &current,
            WasmAbiOutcomeEnvelope::Ok {
                value: WasmAbiValue::String("stable-winner".to_string()),
            },
            None,
        )
        .expect("final winner join should succeed");
    assert!(winner.is_ok());

    dispatcher
        .close_scoped_runtime(&scope, &runtime, None)
        .expect("structured teardown should succeed");

    let diagnostics = dispatcher.diagnostic_snapshot();
    assert!(
        diagnostics.is_clean(),
        "restart churn must leave no leaks: {:?}",
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

    let mut pending_cancelled_joins = 0usize;
    for event in events {
        if event.symbol == WasmAbiSymbol::TaskCancel {
            pending_cancelled_joins += 1;
        } else if event.symbol == WasmAbiSymbol::TaskJoin && pending_cancelled_joins > 0 {
            pending_cancelled_joins -= 1;
        }
    }
    assert_eq!(
        pending_cancelled_joins, 0,
        "every cancel event should be balanced by a join during restart churn"
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LifecycleChaosSnapshot {
    event_signatures: Vec<String>,
    spawn_count: usize,
    cancel_count: usize,
    join_count: usize,
    pending_cancelled_joins: usize,
}

fn spawn_lifecycle_task(
    dispatcher: &mut WasmExportDispatcher,
    scope: WasmHandleRef,
    label: String,
) -> WasmHandleRef {
    dispatcher
        .spawn(WasmTaskSpawnBuilder::new(scope).label(label), None)
        .expect("lifecycle-chaos task spawn should succeed")
}

fn cancel_and_join_lifecycle_task(
    dispatcher: &mut WasmExportDispatcher,
    task: WasmHandleRef,
    phase: &str,
    correlation: &str,
) {
    dispatcher
        .task_cancel(
            &WasmTaskCancelRequest {
                task,
                kind: phase.to_string(),
                message: Some(correlation.to_string()),
            },
            None,
        )
        .expect("lifecycle chaos cancellation should succeed");

    let cancelled = WasmAbiOutcomeEnvelope::Cancelled {
        cancellation: WasmAbiCancellation {
            kind: phase.to_string(),
            phase: "completed".to_string(),
            origin_region: "react-use-task".to_string(),
            origin_task: None,
            timestamp_nanos: 0,
            message: Some(correlation.to_string()),
            truncated: false,
        },
    };
    let outcome = dispatcher
        .task_join(&task, cancelled, None)
        .expect("cancelled lifecycle-chaos task should join cleanly");
    assert!(outcome.is_cancelled());
}

fn lifecycle_event_signatures(dispatcher: &WasmExportDispatcher) -> Vec<String> {
    dispatcher
        .event_log()
        .events()
        .iter()
        .map(|event| {
            format!(
                "{}:{:?}->{:?}",
                event.symbol.as_str(),
                event.state_from,
                event.state_to
            )
        })
        .collect()
}

fn lifecycle_event_counts(dispatcher: &WasmExportDispatcher) -> (usize, usize, usize, usize) {
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

    let mut pending_cancelled_joins = 0usize;
    for event in events {
        if event.symbol == WasmAbiSymbol::TaskCancel {
            pending_cancelled_joins += 1;
        } else if event.symbol == WasmAbiSymbol::TaskJoin && pending_cancelled_joins > 0 {
            pending_cancelled_joins -= 1;
        }
    }
    (
        spawn_count,
        cancel_count,
        join_count,
        pending_cancelled_joins,
    )
}

fn run_lifecycle_chaos_scenario() -> LifecycleChaosSnapshot {
    let mut dispatcher = WasmExportDispatcher::new();

    let (runtime_a, scope_a) = dispatcher
        .create_scoped_runtime(Some("react-lifecycle-chaos-a"), None)
        .expect("runtime/scope A creation should succeed");

    let mut current = spawn_lifecycle_task(
        &mut dispatcher,
        scope_a,
        "lifecycle-chaos-initial".to_string(),
    );

    for (phase, kind) in [
        (
            "background_throttle",
            "react.lifecycle.background_throttle.1",
        ),
        (
            "foreground_resume_soft_nav",
            "react.lifecycle.soft_navigation.2",
        ),
        ("tab_suspend_resume", "react.lifecycle.suspend_resume.3"),
        ("hard_navigation_reset", "react.lifecycle.hard_navigation.4"),
    ] {
        cancel_and_join_lifecycle_task(&mut dispatcher, current, phase, kind);
        current =
            spawn_lifecycle_task(&mut dispatcher, scope_a, format!("lifecycle-chaos-{phase}"));
    }

    let final_cancel = WasmAbiOutcomeEnvelope::Cancelled {
        cancellation: WasmAbiCancellation {
            kind: "hard_navigation_reset".to_string(),
            phase: "completed".to_string(),
            origin_region: "react-use-task".to_string(),
            origin_task: None,
            timestamp_nanos: 0,
            message: Some("react.lifecycle.hard_navigation.final".to_string()),
            truncated: false,
        },
    };
    let final_cancelled = dispatcher
        .task_join(&current, final_cancel, None)
        .expect("final hard-navigation cancellation should join");
    assert!(final_cancelled.is_cancelled());

    dispatcher
        .close_scoped_runtime(&scope_a, &runtime_a, None)
        .expect("structured teardown for runtime A should succeed");

    let (runtime_b, scope_b) = dispatcher
        .create_scoped_runtime(Some("react-lifecycle-chaos-b"), None)
        .expect("runtime/scope B creation should succeed");
    let resumed = spawn_lifecycle_task(&mut dispatcher, scope_b, "lifecycle-chaos-resumed".into());

    let resumed_outcome = dispatcher
        .task_join(
            &resumed,
            WasmAbiOutcomeEnvelope::Ok {
                value: WasmAbiValue::String("resumed-winner".to_string()),
            },
            None,
        )
        .expect("resumed winner should join cleanly");
    assert!(resumed_outcome.is_ok());

    dispatcher
        .close_scoped_runtime(&scope_b, &runtime_b, None)
        .expect("structured teardown for runtime B should succeed");

    let diagnostics = dispatcher.diagnostic_snapshot();
    assert!(
        diagnostics.is_clean(),
        "lifecycle chaos scenario must leave no leaks: {:?}",
        diagnostics.as_log_fields()
    );

    let (spawn_count, cancel_count, join_count, pending_cancelled_joins) =
        lifecycle_event_counts(&dispatcher);

    LifecycleChaosSnapshot {
        event_signatures: lifecycle_event_signatures(&dispatcher),
        spawn_count,
        cancel_count,
        join_count,
        pending_cancelled_joins,
    }
}

#[test]
fn lifecycle_background_throttle_suspend_resume_navigation_churn_is_deterministic() {
    let first = run_lifecycle_chaos_scenario();
    let second = run_lifecycle_chaos_scenario();

    assert_eq!(
        first, second,
        "lifecycle chaos scenario should emit deterministic event signatures"
    );
    assert_eq!(first.cancel_count, 4);
    assert_eq!(first.spawn_count, 6);
    assert_eq!(first.join_count, 6);
    assert_eq!(
        first.pending_cancelled_joins, 0,
        "every lifecycle-chaos cancellation must be drained by a join"
    );
}
