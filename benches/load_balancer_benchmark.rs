#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::Arc;

use asupersync::transport::{
    Endpoint, EndpointId, EndpointState, LoadBalanceStrategy, LoadBalancer,
};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn build_endpoints(count: usize, unhealthy_stride: Option<usize>) -> Vec<Arc<Endpoint>> {
    let mut endpoints = Vec::with_capacity(count);
    for i in 0..count {
        let mut endpoint = Endpoint::new(EndpointId::new(i as u64), format!("node-{i}:9000"));
        if unhealthy_stride.is_some_and(|stride| stride > 0 && i % stride == 0) {
            endpoint = endpoint.with_state(EndpointState::Unhealthy);
        }
        endpoints.push(Arc::new(endpoint));
    }
    endpoints
}

fn build_loaded_endpoints(
    count: usize,
    unhealthy_stride: Option<usize>,
    weighted: bool,
) -> Vec<Arc<Endpoint>> {
    let mut endpoints = Vec::with_capacity(count);
    for i in 0..count {
        let mut endpoint = Endpoint::new(EndpointId::new(i as u64), format!("node-{i}:9000"));
        if weighted {
            endpoint = endpoint.with_weight(((i % 7) + 1) as u32);
        }
        if unhealthy_stride.is_some_and(|stride| stride > 0 && i % stride == 0) {
            endpoint = endpoint.with_state(EndpointState::Unhealthy);
        }

        let endpoint = Arc::new(endpoint);
        endpoint
            .active_connections
            .store(((i * 17) % 97) as u32, std::sync::atomic::Ordering::Relaxed);
        endpoints.push(endpoint);
    }
    endpoints
}

fn bench_load_balancer_select_n_random(c: &mut Criterion) {
    let mut group = c.benchmark_group("transport/load_balancer/select_n_random");

    let scenarios = [
        ("all_healthy", None),
        ("mixed_20pct_unhealthy", Some(5usize)),
    ];

    for &endpoint_count in &[8usize, 32, 128, 512] {
        for &(scenario_name, unhealthy_stride) in &scenarios {
            let endpoints = build_endpoints(endpoint_count, unhealthy_stride);
            let available = endpoints
                .iter()
                .filter(|endpoint| endpoint.state().can_receive())
                .count();

            for &fanout in &[1usize, 3, 8] {
                if fanout > available {
                    continue;
                }

                let lb = LoadBalancer::new(LoadBalanceStrategy::Random);
                let bench_id = BenchmarkId::new(
                    format!("{scenario_name}/endpoints={endpoint_count}"),
                    format!("fanout={fanout}"),
                );
                group.throughput(Throughput::Elements(fanout as u64));

                group.bench_with_input(bench_id, &fanout, |b, &fanout| {
                    b.iter(|| {
                        let selected = lb.select_n(black_box(&endpoints), fanout, None);
                        black_box(selected.first().map_or(0, |endpoint| endpoint.id.0));
                        black_box(selected.len())
                    });
                });
            }
        }
    }

    group.finish();
}

fn bench_load_balancer_select_n_ordered(c: &mut Criterion) {
    let mut group = c.benchmark_group("transport/load_balancer/select_n_ordered");

    let scenarios = [
        (
            "least_connections/all_healthy",
            LoadBalanceStrategy::LeastConnections,
            None,
            false,
        ),
        (
            "least_connections/mixed_20pct_unhealthy",
            LoadBalanceStrategy::LeastConnections,
            Some(5usize),
            false,
        ),
        (
            "weighted_least_connections/all_healthy",
            LoadBalanceStrategy::WeightedLeastConnections,
            None,
            true,
        ),
        (
            "weighted_least_connections/mixed_20pct_unhealthy",
            LoadBalanceStrategy::WeightedLeastConnections,
            Some(5usize),
            true,
        ),
    ];

    for &endpoint_count in &[8usize, 32, 128, 512] {
        for &(scenario_name, strategy, unhealthy_stride, weighted) in &scenarios {
            let endpoints = build_loaded_endpoints(endpoint_count, unhealthy_stride, weighted);
            let available = endpoints
                .iter()
                .filter(|endpoint| endpoint.state().can_receive())
                .count();

            for &fanout in &[1usize, 3, 8] {
                if fanout > available {
                    continue;
                }

                let lb = LoadBalancer::new(strategy);
                let bench_id = BenchmarkId::new(
                    format!("{scenario_name}/endpoints={endpoint_count}"),
                    format!("fanout={fanout}"),
                );
                group.throughput(Throughput::Elements(fanout as u64));

                group.bench_with_input(bench_id, &fanout, |b, &fanout| {
                    b.iter(|| {
                        let selected = lb.select_n(black_box(&endpoints), fanout, None);
                        black_box(selected.first().map_or(0, |endpoint| endpoint.id.0));
                        black_box(selected.len())
                    });
                });
            }
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_load_balancer_select_n_random,
    bench_load_balancer_select_n_ordered
);
criterion_main!(benches);
