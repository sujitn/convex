//! Bench for the Z-spread / OAS cashflow PV inner loop.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

fn pv_with_spread(amounts: &[f64], dfs: &[f64], times: &[f64], spread: f64) -> f64 {
    let mut pv = 0.0;
    for i in 0..amounts.len() {
        pv += amounts[i] * dfs[i] * (-spread * times[i]).exp();
    }
    pv
}

fn make_inputs(years: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = (years * 2.0) as usize;
    let mut amounts = vec![5.0 / 2.0; n];
    *amounts.last_mut().unwrap() += 100.0;
    let times: Vec<f64> = (1..=n).map(|k| k as f64 / 2.0).collect();
    let dfs: Vec<f64> = times.iter().map(|&t| (-0.04_f64 * t).exp()).collect();
    (amounts, dfs, times)
}

fn bench_pv_kernel(c: &mut Criterion) {
    let mut group = c.benchmark_group("spread_pv_kernel");

    for &years in &[5.0_f64, 15.0, 30.0] {
        let (amounts, dfs, times) = make_inputs(years);
        let n = amounts.len();

        group.bench_with_input(BenchmarkId::new("single_eval", n), &n, |b, _| {
            b.iter(|| black_box(pv_with_spread(&amounts, &dfs, &times, black_box(0.0050))));
        });

        // 80 evals per outer iter mimics a Brent solve trajectory.
        group.bench_with_input(BenchmarkId::new("brent_envelope_80", n), &n, |b, _| {
            b.iter(|| {
                let mut acc = 0.0;
                let mut z = -0.05;
                for _ in 0..80 {
                    acc += pv_with_spread(&amounts, &dfs, &times, black_box(z));
                    z += 0.0025;
                }
                black_box(acc)
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_pv_kernel);
criterion_main!(benches);
