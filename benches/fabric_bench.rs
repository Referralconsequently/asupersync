//! FABRIC hot-path performance benchmarks.
//!
//! Criterion benchmarks for subject routing, CRDT merge, morphism
//! transform application, sharded routing, and link-cache acceleration.

#![allow(missing_docs)]
#![cfg(feature = "messaging-fabric")]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::Arc;

use asupersync::messaging::SubjectTransform;
use asupersync::messaging::control::{
    CursorCheckpoint, CursorMark, InterestSummary, JoinSemilattice, LagSketch, MembershipRecord,
    MembershipState, MembershipView,
};
use asupersync::messaging::subject::{
    ShardedSublist, Subject, SubjectPattern, Sublist, SublistLinkCache,
};
use asupersync::remote::NodeId;

// ---------------------------------------------------------------------------
// Subject lookup benchmarks
// ---------------------------------------------------------------------------

fn bench_literal_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("subject_lookup");

    let sl = Arc::new(Sublist::new());
    // Subscribe 100 literal patterns
    let guards: Vec<_> = (0..100)
        .map(|i| {
            let pattern = SubjectPattern::new(&format!("orders.region{i}.created"));
            sl.subscribe(&pattern, None)
        })
        .collect();

    let subject = Subject::new("orders.region42.created");
    group.bench_function("literal_cached", |b| {
        b.iter(|| sl.lookup(&subject));
    });

    let wildcard_subject = Subject::new("orders.region99.created");
    group.bench_function("literal_miss", |b| {
        b.iter(|| sl.lookup(&wildcard_subject));
    });

    drop(guards);
    group.finish();
}

fn bench_wildcard_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("subject_wildcard_lookup");

    let sl = Arc::new(Sublist::new());
    let _g1 = sl.subscribe(&SubjectPattern::new("orders.*.created"), None);
    let _g2 = sl.subscribe(&SubjectPattern::new("orders.>"), None);
    let _g3 = sl.subscribe(&SubjectPattern::new(">"), None);

    let subject = Subject::new("orders.region1.created");

    group.bench_function("single_wildcard", |b| {
        b.iter(|| sl.lookup(&subject));
    });

    group.finish();
}

fn bench_link_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("subject_link_cache");

    let sl = Arc::new(Sublist::new());
    let _g = sl.subscribe(&SubjectPattern::new("orders.>"), None);

    let subject = Subject::new("orders.region1.created");

    // Uncached path
    group.bench_function("uncached", |b| {
        b.iter(|| sl.lookup(&subject));
    });

    // Cached path (pre-warm then measure hits)
    let mut link_cache = SublistLinkCache::new(64);
    let _ = sl.lookup_with_link_cache(&subject, &mut link_cache); // warm
    group.bench_function("cached_hit", |b| {
        b.iter(|| {
            let _ = sl.lookup_with_link_cache(&subject, &mut link_cache);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Sharded sublist benchmarks
// ---------------------------------------------------------------------------

fn bench_sharded_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("sharded_sublist");

    for shard_count in [1, 4, 8, 16] {
        let sharded = ShardedSublist::new(shard_count);
        let _g = sharded.subscribe(&SubjectPattern::new(">"), None);

        let subject = Subject::new("orders.region1.created");

        group.bench_with_input(
            BenchmarkId::new("lookup", shard_count),
            &shard_count,
            |b, _| {
                b.iter(|| sharded.lookup(&subject));
            },
        );
    }

    group.finish();
}

fn bench_shard_assignment(c: &mut Criterion) {
    let mut group = c.benchmark_group("shard_assignment");

    let sharded = ShardedSublist::new(8);
    let subject = Subject::new("orders.region1.created");

    group.bench_function("shard_index", |b| {
        b.iter(|| sharded.shard_index_for_subject(&subject));
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// CRDT merge benchmarks
// ---------------------------------------------------------------------------

fn bench_crdt_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("crdt_merge");

    let node_a = NodeId::new("node-a");
    let node_b = NodeId::new("node-b");

    // InterestSummary merge
    {
        let mut a = InterestSummary::default();
        a.subscribe(&node_a, SubjectPattern::new("orders.>"));
        a.subscribe(&node_a, SubjectPattern::new("events.*"));
        let mut b = InterestSummary::default();
        b.subscribe(&node_b, SubjectPattern::new("orders.>"));
        b.subscribe(&node_b, SubjectPattern::new("logs.>"));

        group.bench_function("interest_summary", |bench| {
            bench.iter(|| {
                let mut merged = a.clone();
                merged.merge(&b);
                merged
            });
        });
    }

    // CursorCheckpoint merge
    {
        let mut a = CursorCheckpoint::default();
        a.observe("consumer-1", CursorMark::new(100, 1000, node_a.clone()));
        a.observe("consumer-2", CursorMark::new(200, 2000, node_a.clone()));
        let mut b = CursorCheckpoint::default();
        b.observe("consumer-1", CursorMark::new(150, 1500, node_b.clone()));
        b.observe("consumer-3", CursorMark::new(50, 500, node_b.clone()));

        group.bench_function("cursor_checkpoint", |bench| {
            bench.iter(|| {
                let mut merged = a.clone();
                merged.merge(&b);
                merged
            });
        });
    }

    // LagSketch merge
    {
        let mut a = LagSketch::default();
        for i in 0..10 {
            a.observe(&node_a, i * 100);
        }
        let mut b = LagSketch::default();
        for i in 0..10 {
            b.observe(&node_b, i * 150);
        }

        group.bench_function("lag_sketch", |bench| {
            bench.iter(|| {
                let mut merged = a.clone();
                merged.merge(&b);
                merged
            });
        });
    }

    // MembershipView merge
    {
        let mut a = MembershipView::default();
        a.observe(
            node_a.clone(),
            MembershipRecord::new(10, MembershipState::Healthy, 1000, 500),
        );
        let mut b = MembershipView::default();
        b.observe(
            node_b.clone(),
            MembershipRecord::new(12, MembershipState::Healthy, 1200, 300),
        );

        group.bench_function("membership_view", |bench| {
            bench.iter(|| {
                let mut merged = a.clone();
                merged.merge(&b);
                merged
            });
        });
    }

    group.finish();
}

fn bench_crdt_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("crdt_delta");

    let node_a = NodeId::new("node-a");

    // InterestSummary delta
    {
        let baseline = InterestSummary::default();
        let mut updated = InterestSummary::default();
        updated.subscribe(&node_a, SubjectPattern::new("orders.>"));
        updated.subscribe(&node_a, SubjectPattern::new("events.*"));

        group.bench_function("interest_summary_delta", |bench| {
            bench.iter(|| updated.delta(&baseline));
        });

        let delta = updated.delta(&baseline);
        group.bench_function("interest_summary_apply", |bench| {
            bench.iter(|| {
                let mut applied = baseline.clone();
                applied.apply_delta(&delta);
                applied
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Morphism transform benchmarks
// ---------------------------------------------------------------------------

fn bench_transform_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_apply");

    let tokens = vec![
        "orders".to_string(),
        "region1".to_string(),
        "created".to_string(),
    ];

    // Identity
    group.bench_function("identity", |b| {
        let t = SubjectTransform::Identity;
        b.iter(|| t.apply_tokens(&tokens));
    });

    // RenamePrefix
    group.bench_function("rename_prefix", |b| {
        let t = SubjectTransform::RenamePrefix {
            from: SubjectPattern::new("orders"),
            to: SubjectPattern::new("processed"),
        };
        b.iter(|| t.apply_tokens(&tokens));
    });

    // DeterministicHash
    group.bench_function("deterministic_hash", |b| {
        let t = SubjectTransform::DeterministicHash {
            buckets: 16,
            source_indices: vec![1, 2],
        };
        b.iter(|| t.apply_tokens(&tokens));
    });

    // HashPartition
    group.bench_function("hash_partition", |b| {
        let t = SubjectTransform::HashPartition { buckets: 8 };
        b.iter(|| t.apply_tokens(&tokens));
    });

    // Compose (rename + hash)
    group.bench_function("compose_two", |b| {
        let t = SubjectTransform::Compose {
            steps: vec![
                SubjectTransform::RenamePrefix {
                    from: SubjectPattern::new("orders"),
                    to: SubjectPattern::new("processed"),
                },
                SubjectTransform::DeterministicHash {
                    buckets: 8,
                    source_indices: vec![1],
                },
            ],
        };
        b.iter(|| t.apply_tokens(&tokens));
    });

    // LeftExtract
    group.bench_function("left_extract", |b| {
        let t = SubjectTransform::LeftExtract { index: 2, len: 3 };
        b.iter(|| t.apply_tokens(&tokens));
    });

    // SplitSlice
    group.bench_function("split_slice", |b| {
        let tokens_with_delim = vec!["key-val".to_string(), "data".to_string()];
        let t = SubjectTransform::SplitSlice {
            index: 1,
            delimiter: "-".to_string(),
            start: 0,
            len: 1,
        };
        b.iter(|| t.apply_tokens(&tokens_with_delim));
    });

    group.finish();
}

fn bench_transform_invertibility(c: &mut Criterion) {
    let mut group = c.benchmark_group("transform_invertibility");

    let rename = SubjectTransform::RenamePrefix {
        from: SubjectPattern::new("orders"),
        to: SubjectPattern::new("processed"),
    };

    group.bench_function("is_invertible_rename", |b| {
        b.iter(|| rename.is_invertible());
    });

    let compose = SubjectTransform::Compose {
        steps: vec![
            SubjectTransform::RenamePrefix {
                from: SubjectPattern::new("a"),
                to: SubjectPattern::new("b"),
            },
            SubjectTransform::RenamePrefix {
                from: SubjectPattern::new("b"),
                to: SubjectPattern::new("c"),
            },
        ],
    };

    group.bench_function("is_invertible_compose", |b| {
        b.iter(|| compose.is_invertible());
    });

    group.bench_function("inverse_compose", |b| {
        b.iter(|| compose.inverse());
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Pattern matching benchmarks
// ---------------------------------------------------------------------------

fn bench_pattern_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_matching");

    let literal = SubjectPattern::new("orders.region1.created");
    let wildcard = SubjectPattern::new("orders.*.created");
    let tail = SubjectPattern::new("orders.>");
    let subject = Subject::new("orders.region1.created");

    group.bench_function("literal_matches", |b| {
        b.iter(|| literal.matches(&subject));
    });

    group.bench_function("single_wildcard_matches", |b| {
        b.iter(|| wildcard.matches(&subject));
    });

    group.bench_function("tail_wildcard_matches", |b| {
        b.iter(|| tail.matches(&subject));
    });

    let p1 = SubjectPattern::new("orders.*.created");
    let p2 = SubjectPattern::new("orders.region1.*");
    group.bench_function("overlaps", |b| {
        b.iter(|| p1.overlaps(&p2));
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Subscribe/unsubscribe throughput
// ---------------------------------------------------------------------------

fn bench_subscribe_unsubscribe(c: &mut Criterion) {
    let mut group = c.benchmark_group("subscribe_unsubscribe");

    let sl = Arc::new(Sublist::new());
    let pattern = SubjectPattern::new("orders.region1.created");

    group.bench_function("subscribe_then_drop", |b| {
        b.iter(|| {
            let guard = sl.subscribe(&pattern, None);
            drop(guard);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Scaled lookup benchmarks
// ---------------------------------------------------------------------------

fn bench_scaled_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaled_lookup");

    for sub_count in [10, 100, 1000] {
        let sl = Arc::new(Sublist::new());
        let guards: Vec<_> = (0..sub_count)
            .map(|i| {
                let pattern = SubjectPattern::new(&format!("svc{i}.events.created"));
                sl.subscribe(&pattern, None)
            })
            .collect();

        let subject = Subject::new("svc0.events.created");

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("literal", sub_count),
            &sub_count,
            |b, _| {
                b.iter(|| sl.lookup(&subject));
            },
        );

        drop(guards);
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_literal_lookup,
    bench_wildcard_lookup,
    bench_link_cache,
    bench_sharded_lookup,
    bench_shard_assignment,
    bench_crdt_merge,
    bench_crdt_delta,
    bench_transform_apply,
    bench_transform_invertibility,
    bench_pattern_matching,
    bench_subscribe_unsubscribe,
    bench_scaled_lookup,
);

criterion_main!(benches);
