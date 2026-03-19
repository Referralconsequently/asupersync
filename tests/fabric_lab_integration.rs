//! LabRuntime integration coverage for the current FABRIC subject-cell foundation.
#![cfg(feature = "messaging-fabric")]

use asupersync::lab::{LabConfig, LabRuntime};
use asupersync::messaging::fabric::{
    CellEpoch, CellTemperature, DataCapsule, NodeRole, NormalizationPolicy, ObservedCellLoad,
    PlacementPolicy, RebalanceBudget, RebalancePlan, RepairPolicy, ReplySpaceCompactionPolicy,
    StewardCandidate, StorageClass, SubjectCell, SubjectPattern, SubjectPrefixMorphism,
};
use asupersync::remote::NodeId;
use asupersync::runtime::yield_now;
use asupersync::types::Budget;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CellSnapshot {
    input_subject: String,
    canonical_partition: String,
    cell_id: u128,
    steward_set: Vec<String>,
    active_sequencer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RebalanceSnapshot {
    input_subject: String,
    next_temperature: CellTemperature,
    next_stewards: Vec<String>,
    added_stewards: Vec<String>,
    removed_stewards: Vec<String>,
}

fn candidate(
    name: &str,
    domain: &str,
    storage_class: StorageClass,
    latency_millis: u32,
) -> StewardCandidate {
    StewardCandidate::new(NodeId::new(name), domain)
        .with_role(NodeRole::Steward)
        .with_role(NodeRole::RepairWitness)
        .with_storage_class(storage_class)
        .with_latency_millis(latency_millis)
}

fn role_mixed_candidates() -> Vec<StewardCandidate> {
    vec![
        candidate("node-a", "rack-a", StorageClass::Durable, 5),
        candidate("node-b", "rack-b", StorageClass::Standard, 6),
        candidate("node-c", "rack-c", StorageClass::Standard, 7),
        StewardCandidate::new(NodeId::new("observer"), "rack-d").with_role(NodeRole::Subscriber),
        StewardCandidate::new(NodeId::new("bridge"), "rack-e").with_role(NodeRole::Bridge),
    ]
}

fn alias_policy() -> PlacementPolicy {
    PlacementPolicy {
        normalization: NormalizationPolicy {
            morphisms: vec![
                SubjectPrefixMorphism::new("svc.orders", "orders").expect("svc -> orders"),
            ],
            reply_space_policy: ReplySpaceCompactionPolicy {
                enabled: true,
                preserve_segments: 3,
            },
        },
        ..PlacementPolicy::default()
    }
}

fn hot_rebalance_policy() -> PlacementPolicy {
    PlacementPolicy {
        cold_stewards: 1,
        warm_stewards: 2,
        hot_stewards: 3,
        candidate_pool_size: 5,
        rebalance_budget: RebalanceBudget {
            max_steward_changes: 2,
        },
        normalization: NormalizationPolicy {
            morphisms: vec![
                SubjectPrefixMorphism::new("svc.orders", "orders").expect("svc -> orders"),
            ],
            reply_space_policy: ReplySpaceCompactionPolicy {
                enabled: true,
                preserve_segments: 3,
            },
        },
        ..PlacementPolicy::default()
    }
}

fn snapshot_cell(cell: SubjectCell, input_subject: &str) -> CellSnapshot {
    CellSnapshot {
        input_subject: input_subject.to_string(),
        canonical_partition: cell.subject_partition.canonical_key(),
        cell_id: cell.cell_id.raw(),
        steward_set: cell
            .steward_set
            .into_iter()
            .map(|node| node.as_str().to_string())
            .collect(),
        active_sequencer: cell
            .control_capsule
            .active_sequencer
            .map(|node| node.as_str().to_string()),
    }
}

fn snapshot_rebalance(plan: RebalancePlan, input_subject: &str) -> RebalanceSnapshot {
    RebalanceSnapshot {
        input_subject: input_subject.to_string(),
        next_temperature: plan.next_temperature,
        next_stewards: plan
            .next_stewards
            .into_iter()
            .map(|node| node.as_str().to_string())
            .collect(),
        added_stewards: plan
            .added_stewards
            .into_iter()
            .map(|node| node.as_str().to_string())
            .collect(),
        removed_stewards: plan
            .removed_stewards
            .into_iter()
            .map(|node| node.as_str().to_string())
            .collect(),
    }
}

fn run_subject_cell_scenario(seed: u64, inputs: &[&str]) -> (Vec<CellSnapshot>, u64) {
    let mut runtime = LabRuntime::new(LabConfig::new(seed).max_steps(5_000));
    let region = runtime.state.create_root_region(Budget::INFINITE);
    let results = Arc::new(Mutex::new(Vec::new()));
    let candidates = Arc::new(role_mixed_candidates());
    let policy = Arc::new(alias_policy());

    for input in inputs {
        let input = (*input).to_string();
        let results = Arc::clone(&results);
        let candidates = Arc::clone(&candidates);
        let policy = Arc::clone(&policy);
        let (task_id, _handle) = runtime
            .state
            .create_task(region, Budget::INFINITE, async move {
                yield_now().await;
                let pattern = SubjectPattern::parse(&input).expect("valid subject pattern");
                yield_now().await;
                let cell = SubjectCell::new(
                    &pattern,
                    CellEpoch::new(41, 7),
                    &candidates,
                    &policy,
                    RepairPolicy::default(),
                    DataCapsule {
                        temperature: CellTemperature::Warm,
                        retained_message_blocks: 4,
                    },
                )
                .expect("cell should build");
                yield_now().await;
                results
                    .lock()
                    .expect("results lock")
                    .push(snapshot_cell(cell, &input));
            })
            .expect("create task");
        runtime.scheduler.lock().schedule(task_id, 0);
    }

    runtime.run_until_quiescent();
    let violations = runtime.check_invariants();
    let pending_obligations = runtime.state.pending_obligation_count();
    assert!(
        runtime.is_quiescent(),
        "runtime should quiesce after subject scenario"
    );
    assert_eq!(
        pending_obligations, 0,
        "subject scenario should not leave pending obligations"
    );
    assert!(
        violations.is_empty(),
        "subject scenario should not violate lab invariants: {violations:?}"
    );

    let mut snapshots = results.lock().expect("results lock").clone();
    snapshots.sort_unstable_by(|left, right| left.input_subject.cmp(&right.input_subject));
    (snapshots, runtime.steps())
}

fn run_rebalance_scenario(seed: u64, inputs: &[&str]) -> (Vec<RebalanceSnapshot>, u64) {
    let mut runtime = LabRuntime::new(LabConfig::new(seed).max_steps(5_000));
    let region = runtime.state.create_root_region(Budget::INFINITE);
    let results = Arc::new(Mutex::new(Vec::new()));
    let candidates = Arc::new(role_mixed_candidates());
    let policy = Arc::new(hot_rebalance_policy());

    for input in inputs {
        let input = (*input).to_string();
        let results = Arc::clone(&results);
        let candidates = Arc::clone(&candidates);
        let policy = Arc::clone(&policy);
        let (task_id, _handle) = runtime
            .state
            .create_task(region, Budget::INFINITE, async move {
                yield_now().await;
                let pattern = SubjectPattern::parse(&input).expect("valid subject pattern");
                let current = vec![NodeId::new("node-a")];
                yield_now().await;
                let plan = policy
                    .plan_rebalance(
                        &pattern,
                        &candidates,
                        &current,
                        CellTemperature::Cold,
                        ObservedCellLoad::new(2_048),
                    )
                    .expect("rebalance plan");
                yield_now().await;
                results
                    .lock()
                    .expect("results lock")
                    .push(snapshot_rebalance(plan, &input));
            })
            .expect("create task");
        runtime.scheduler.lock().schedule(task_id, 0);
    }

    runtime.run_until_quiescent();
    let violations = runtime.check_invariants();
    let pending_obligations = runtime.state.pending_obligation_count();
    assert!(
        runtime.is_quiescent(),
        "runtime should quiesce after rebalance scenario"
    );
    assert_eq!(
        pending_obligations, 0,
        "rebalance scenario should not leave pending obligations"
    );
    assert!(
        violations.is_empty(),
        "rebalance scenario should not violate lab invariants: {violations:?}"
    );

    let mut snapshots = results.lock().expect("results lock").clone();
    snapshots.sort_unstable_by(|left, right| left.input_subject.cmp(&right.input_subject));
    (snapshots, runtime.steps())
}

#[test]
fn subject_cell_replay_is_deterministic_across_seeded_lab_runs() {
    let inputs = [
        "orders.created",
        "svc.orders.created",
        "orders.updated",
        "svc.orders.updated",
        "_INBOX.orders.region.instance.123",
    ];

    let (first, first_steps) = run_subject_cell_scenario(0x5EED_FAB1, &inputs);
    let (second, second_steps) = run_subject_cell_scenario(0x5EED_FAB1, &inputs);

    assert_eq!(
        first, second,
        "same seed should yield identical cell snapshots"
    );
    assert_eq!(
        first_steps, second_steps,
        "same seed should yield identical scheduler step counts"
    );
}

#[test]
fn concurrent_alias_subjects_converge_to_one_canonical_cell() {
    let inputs = [
        "orders.created",
        "svc.orders.created",
        "svc.orders.created",
        "orders.created",
    ];

    let (snapshots, _) = run_subject_cell_scenario(0xA11A_5EED, &inputs);
    let canonical = snapshots
        .iter()
        .map(|snapshot| (snapshot.canonical_partition.clone(), snapshot.cell_id))
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        canonical.len(),
        1,
        "all aliases should collapse to the same cell"
    );
    assert!(
        snapshots
            .iter()
            .all(|snapshot| snapshot.active_sequencer == snapshot.steward_set.first().cloned()),
        "active sequencer should stay aligned with the first steward"
    );
}

#[test]
fn reply_space_subjects_compact_to_a_shared_cell_under_lab_runtime() {
    let inputs = [
        "_INBOX.orders.region.instance.123",
        "_INBOX.orders.region.instance.456",
        "_INBOX.orders.region.instance.789",
    ];

    let (snapshots, _) = run_subject_cell_scenario(0xA11A_5E12, &inputs);
    let canonical_partitions = snapshots
        .iter()
        .map(|snapshot| snapshot.canonical_partition.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let cell_ids = snapshots
        .iter()
        .map(|snapshot| snapshot.cell_id)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        canonical_partitions,
        std::collections::BTreeSet::from(["_INBOX.orders.region.>".to_string()]),
        "reply-space subjects should compact before placement"
    );
    assert_eq!(
        cell_ids.len(),
        1,
        "compacted reply-space subjects should share one cell"
    );
}

#[test]
fn concurrent_placement_filters_non_steward_roles() {
    let inputs = ["orders.created", "orders.updated", "orders.deleted"];
    let (snapshots, _) = run_subject_cell_scenario(0xCA11_AB1E, &inputs);

    for snapshot in snapshots {
        assert!(
            snapshot
                .steward_set
                .iter()
                .all(|node| node != "observer" && node != "bridge"),
            "non-steward roles must never appear in steward placement"
        );
    }
}

#[test]
fn rebalance_planning_stays_deterministic_for_alias_inputs() {
    let inputs = [
        "orders.created",
        "svc.orders.created",
        "orders.updated",
        "svc.orders.updated",
    ];

    let (first, first_steps) = run_rebalance_scenario(0xB16B_00B5, &inputs);
    let (second, second_steps) = run_rebalance_scenario(0xB16B_00B5, &inputs);

    assert_eq!(
        first, second,
        "same seed should yield identical rebalance plans"
    );
    assert_eq!(
        first_steps, second_steps,
        "same seed should yield identical rebalance scheduler steps"
    );
    assert!(
        first
            .iter()
            .all(|snapshot| snapshot.next_temperature == CellTemperature::Hot),
        "hot observed load should drive the cell into the hot tier"
    );
}

#[test]
fn rebalance_aliases_choose_the_same_hot_steward_set() {
    let inputs = ["orders.created", "svc.orders.created"];
    let (snapshots, _) = run_rebalance_scenario(0x600D_F11E, &inputs);

    assert_eq!(
        snapshots.len(),
        2,
        "expected both alias inputs to produce a plan"
    );
    assert_eq!(
        snapshots[0].next_stewards, snapshots[1].next_stewards,
        "alias subjects should rebalance to the same steward set after normalization"
    );
    assert!(
        snapshots
            .iter()
            .all(|snapshot| snapshot.added_stewards.len() <= 2),
        "rebalance budget should bound steward churn per planning step"
    );
}
