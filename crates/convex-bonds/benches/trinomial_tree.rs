//! Bench for `TrinomialTree::price()` backward induction.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use convex_bonds::options::TrinomialTree;

fn bench_backward_induction(c: &mut Criterion) {
    let mut group = c.benchmark_group("trinomial_backward_induction");
    let flat = |_: f64| 0.05_f64;

    for &(steps, maturity) in &[(50usize, 5.0_f64), (100, 10.0), (200, 20.0)] {
        let tree = TrinomialTree::build_hull_white(flat, 0.03, 0.01, maturity, steps);
        let n = tree.steps;

        // Precompute coupon-layer mask once; semi-annual schedule mapped onto
        // the lattice index. `coupons[i] == true` means a 2.5 coupon falls on
        // layer `i`.
        let coupons_per_year = 2.0_f64;
        let n_coupons = (maturity * coupons_per_year) as usize;
        let mut coupons = vec![false; n + 1];
        for k in 1..=n_coupons {
            let idx = (k as f64 / (maturity * coupons_per_year) * n as f64) as usize;
            if idx <= n {
                coupons[idx] = true;
            }
        }

        let cashflow_at = |i: usize| -> f64 {
            if i == n {
                102.5
            } else if coupons[i] {
                2.5
            } else {
                0.0
            }
        };
        let no_call = |_: usize| -> Option<f64> { None };

        group.bench_with_input(BenchmarkId::new("straight", steps), &steps, |b, _| {
            b.iter(|| black_box(tree.price(black_box(0.0050), cashflow_at, no_call)));
        });

        let cap = 100.0;
        let call_start = n / 2;
        let call_capped = |i: usize| -> Option<f64> {
            if (call_start..n).contains(&i) {
                Some(cap)
            } else {
                None
            }
        };

        group.bench_with_input(BenchmarkId::new("callable", steps), &steps, |b, _| {
            b.iter(|| black_box(tree.price(black_box(0.0050), cashflow_at, call_capped)));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_backward_induction);
criterion_main!(benches);
