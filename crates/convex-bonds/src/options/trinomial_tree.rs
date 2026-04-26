//! Hagan-Brace trinomial tree for HW1F. Matches QL's
//! `TreeCallableFixedRateBondEngine` lattice (Brigo-Mercurio §3.7).
//! Supports non-uniform time grids so events land on layers exactly.

#[derive(Debug, Clone, Copy)]
struct Branching {
    k: i32,
    pu: f64,
    pm: f64,
    pd: f64,
}

/// Branching from `(j, dx_in)` at layer i to layer i+1 (spacing `dx_out`),
/// with mean-reversion factor `m = exp(-a·dt[i])`. Boundaries at ±j_max use
/// the alternative {k+1, k, k-1} layout to keep probabilities non-negative.
fn branching_at(j: i32, j_max: i32, m: f64, dx_in: f64, dx_out: f64) -> Branching {
    let target = j as f64 * (dx_in / dx_out) * m;
    if j == j_max {
        let k = j_max - 1;
        let eta = target - k as f64;
        Branching {
            k,
            pu: 7.0 / 6.0 + (eta * eta + 3.0 * eta) / 2.0,
            pm: -1.0 / 3.0 - eta * eta - 2.0 * eta,
            pd: 1.0 / 6.0 + (eta * eta + eta) / 2.0,
        }
    } else if j == -j_max {
        let k = -j_max + 1;
        let eta = target - k as f64;
        Branching {
            k,
            pu: 1.0 / 6.0 + (eta * eta - eta) / 2.0,
            pm: -1.0 / 3.0 - eta * eta + 2.0 * eta,
            pd: 7.0 / 6.0 + (eta * eta - 3.0 * eta) / 2.0,
        }
    } else {
        let k = target.round() as i32;
        let eta = target - k as f64;
        Branching {
            k,
            pu: 1.0 / 6.0 + (eta * eta + eta) / 2.0,
            pm: 2.0 / 3.0 - eta * eta,
            pd: 1.0 / 6.0 + (eta * eta - eta) / 2.0,
        }
    }
}

/// Non-uniform grid hitting every `mandatory` time exactly, padded with
/// roughly `desired_steps` intermediate points. Mirrors QL's
/// `TimeGrid(begin, end, steps, mandatoryTimes)`.
#[must_use]
pub fn build_event_grid(end: f64, mandatory: &[f64], desired_steps: usize) -> Vec<f64> {
    assert!(end > 0.0, "grid endpoint must be positive");
    assert!(desired_steps >= 1, "grid must have at least one step");

    let tol = end * 1e-12;
    let mut sorted: Vec<f64> = mandatory
        .iter()
        .copied()
        .filter(|&t| t > tol && t < end - tol)
        .collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted.dedup_by(|a, b| (*a - *b).abs() < tol);

    let mut anchors = Vec::with_capacity(sorted.len() + 2);
    anchors.push(0.0);
    anchors.extend(sorted);
    anchors.push(end);

    let dt_target = end / desired_steps as f64;
    let mut grid = Vec::with_capacity(desired_steps + anchors.len());
    grid.push(0.0);
    for w in anchors.windows(2) {
        let span = w[1] - w[0];
        let n_sub = ((span / dt_target).ceil() as usize).max(1);
        let sub_dt = span / n_sub as f64;
        for k in 1..=n_sub {
            grid.push(w[0] + sub_dt * k as f64);
        }
    }
    *grid.last_mut().unwrap() = end;
    grid
}

#[derive(Debug, Clone)]
pub struct TrinomialTree {
    pub steps: usize,
    /// `times[0] = 0`, `times[steps] = T`. Length steps+1.
    pub times: Vec<f64>,
    /// `dt[i] = times[i+1] - times[i]`. Length steps.
    pub dt: Vec<f64>,
    /// j-spacing at layer i: `dx[i+1] = σ·√(3·dt[i])`. Length steps+1.
    pub dx: Vec<f64>,
    pub j_max: i32,
    /// `m[i] = exp(-a·dt[i])`. Length steps.
    m: Vec<f64>,
    /// Arrow-Debreu fitting shift; `alpha[i]` reprices `D(times[i+1])`.
    pub alpha: Vec<f64>,
}

impl TrinomialTree {
    /// Uniform grid with `steps` intervals over `[0, maturity]`.
    #[must_use]
    pub fn build_hull_white<F>(
        zero_rates: F,
        a: f64,
        sigma: f64,
        maturity: f64,
        steps: usize,
    ) -> Self
    where
        F: Fn(f64) -> f64,
    {
        assert!(steps > 0, "trinomial tree needs at least 1 step");
        let times: Vec<f64> = (0..=steps)
            .map(|i| maturity * i as f64 / steps as f64)
            .collect();
        Self::build_hull_white_on_grid(zero_rates, a, sigma, &times)
    }

    /// Strictly-increasing grid starting at 0. Use with `build_event_grid`
    /// to align cashflow and call dates onto layers exactly.
    #[must_use]
    pub fn build_hull_white_on_grid<F>(zero_rates: F, a: f64, sigma: f64, times: &[f64]) -> Self
    where
        F: Fn(f64) -> f64,
    {
        assert!(a > 0.0, "Hull-White mean reversion must be positive");
        assert!(sigma > 0.0, "Hull-White volatility must be positive");
        assert!(times.len() >= 2, "time grid needs at least 2 points");
        assert!(times[0].abs() < 1e-12, "time grid must start at 0");

        let steps = times.len() - 1;
        let mut dt = Vec::with_capacity(steps);
        let mut dx = Vec::with_capacity(steps + 1);
        let mut m = Vec::with_capacity(steps);

        for i in 0..steps {
            let d = times[i + 1] - times[i];
            assert!(d > 0.0, "time grid must be strictly increasing");
            dt.push(d);
            m.push((-a * d).exp());
        }
        // dx[0] is never used in arithmetic that affects results (j=0 at the
        // root); set it equal to dx[1] so branching_at can divide unconditionally.
        let dx1 = sigma * (3.0 * dt[0]).sqrt();
        dx.push(dx1);
        dx.push(dx1);
        for &d in &dt[1..] {
            dx.push(sigma * (3.0 * d).sqrt());
        }

        // 0.184/(a·Δt) is the minimum j_max for non-negative boundary
        // probabilities (Hagan-Brace); take the worst-case dt so every
        // layer is safe.
        let min_dt = dt.iter().copied().fold(f64::INFINITY, f64::min);
        let j_max = ((0.184 / (a * min_dt)).ceil().max(1.0)) as i32;

        // Forward induction with Arrow-Debreu prices Q[i][j+j_max].
        let row_len = (2 * j_max + 1) as usize;
        let mut alpha = vec![0.0f64; steps];
        let mut q = vec![0.0f64; row_len];
        q[j_max as usize] = 1.0;
        let mut q_next = vec![0.0f64; row_len];

        for (i, alpha_i) in alpha.iter_mut().enumerate().take(steps) {
            let band = (i as i32).min(j_max);
            let dx_i = dx[i];
            let dt_i = dt[i];

            // exp(-α[i]·dt[i]) · Σ_j Q[i][j] · exp(-x_j·dt[i]) = D(t_{i+1})
            let mut numer = 0.0f64;
            for j in -band..=band {
                let qij = q[(j + j_max) as usize];
                if qij > 0.0 {
                    numer += qij * (-(j as f64 * dx_i) * dt_i).exp();
                }
            }
            let d_next = (-zero_rates(times[i + 1]) * times[i + 1]).exp();
            *alpha_i = (numer / d_next).max(1e-300).ln() / dt_i;

            // Propagate Q[i] → Q[i+1].
            q_next.fill(0.0);
            for j in -band..=band {
                let qij = q[(j + j_max) as usize];
                if qij == 0.0 {
                    continue;
                }
                let br = branching_at(j, j_max, m[i], dx_i, dx[i + 1]);
                let weight = qij * (-(*alpha_i + j as f64 * dx_i) * dt_i).exp();
                let band_next = ((i + 1) as i32).min(j_max);
                for (offset, p) in [(1, br.pu), (0, br.pm), (-1, br.pd)] {
                    let child = br.k + offset;
                    if child.abs() <= band_next {
                        q_next[(child + j_max) as usize] += weight * p;
                    }
                }
            }
            std::mem::swap(&mut q, &mut q_next);
        }

        Self {
            steps,
            times: times.to_vec(),
            dt,
            dx,
            j_max,
            m,
            alpha,
        }
    }

    /// Layer whose time matches `t` within 1e-9 years, else `None`.
    #[must_use]
    pub fn step_at_time(&self, t: f64) -> Option<usize> {
        let tol = 1e-9;
        let idx = self.times.partition_point(|&ti| ti < t - tol);
        self.times
            .get(idx)
            .filter(|ti| (**ti - t).abs() < tol)
            .map(|_| idx)
    }

    /// Backward-induction PV. `cashflow_at(i)` is added at layer i;
    /// `call_price_at(i)` is the dirty cap (`clean + accrued`). Holder
    /// value at a callable layer is `min(continuation, cap) + cashflow` —
    /// the cap bounds the continuation, not the post-coupon dirty value
    /// (matches QL `CouponAdjustment::post` semantics). `oas` shifts the
    /// short rate before discounting.
    #[must_use]
    pub fn price<C, K>(&self, oas: f64, mut cashflow_at: C, mut call_price_at: K) -> f64
    where
        C: FnMut(usize) -> f64,
        K: FnMut(usize) -> Option<f64>,
    {
        let row_len = (2 * self.j_max + 1) as usize;
        let n = self.steps;

        let terminal = cashflow_at(n);
        let mut values = vec![terminal; row_len];
        let mut new_values = vec![0.0f64; row_len];

        for i in (0..n).rev() {
            let band = (i as i32).min(self.j_max);
            let dx_i = self.dx[i];
            let dx_next = self.dx[i + 1];
            let dt_i = self.dt[i];
            let m_i = self.m[i];
            let cap_at_i = if i > 0 { call_price_at(i) } else { None };
            let cf_i = cashflow_at(i);
            new_values.fill(0.0);
            for j in -band..=band {
                let br = branching_at(j, self.j_max, m_i, dx_i, dx_next);
                let r = self.alpha[i] + j as f64 * dx_i + oas;
                let df = (-r * dt_i).exp();
                let v_up = values[(br.k + 1 + self.j_max) as usize];
                let v_mid = values[(br.k + self.j_max) as usize];
                let v_down = values[(br.k - 1 + self.j_max) as usize];
                let cont = df * (br.pu * v_up + br.pm * v_mid + br.pd * v_down);
                let capped = cap_at_i.map_or(cont, |cap| cont.min(cap));
                new_values[(j + self.j_max) as usize] = capped + cf_i;
            }
            std::mem::swap(&mut values, &mut new_values);
        }

        values[self.j_max as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(rate: f64) -> impl Fn(f64) -> f64 {
        move |_t: f64| rate
    }

    #[test]
    fn flat_curve_zero_coupon_recovers_discount() {
        let tree = TrinomialTree::build_hull_white(flat(0.05), 0.03, 0.01, 1.0, 50);
        let n = tree.steps;
        let pv = tree.price(0.0, |i| if i == n { 100.0 } else { 0.0 }, |_| None);
        assert!((pv - 100.0_f64 * (-0.05_f64).exp()).abs() < 5e-6);
    }

    #[test]
    fn upward_curve_zero_coupon_recovers_discount() {
        let zero = |t: f64| 0.03 + 0.01 * t;
        let tree = TrinomialTree::build_hull_white(zero, 0.03, 0.008, 5.0, 60);
        let n = tree.steps;
        let pv = tree.price(0.0, |i| if i == n { 100.0 } else { 0.0 }, |_| None);
        assert!((pv - 100.0_f64 * (-zero(5.0) * 5.0).exp()).abs() < 5e-5);
    }

    #[test]
    fn intermediate_step_recovers_curve() {
        let zero = |t: f64| 0.04 - 0.001 * t;
        let tree = TrinomialTree::build_hull_white(zero, 0.05, 0.012, 5.0, 100);
        let half = tree.steps / 2;
        let pv = tree.price(0.0, |i| if i == half { 100.0 } else { 0.0 }, |_| None);
        let t_half = tree.dt.iter().take(half).sum::<f64>();
        assert!((pv - 100.0_f64 * (-zero(t_half) * t_half).exp()).abs() < 1e-3);
    }

    #[test]
    fn oas_shifts_price_consistently() {
        let tree = TrinomialTree::build_hull_white(flat(0.04), 0.03, 0.01, 5.0, 60);
        let n = tree.steps;
        let pv0 = tree.price(0.0, |i| if i == n { 100.0 } else { 0.0 }, |_| None);
        let pv100 = tree.price(0.01, |i| if i == n { 100.0 } else { 0.0 }, |_| None);
        // 100bp OAS over 5Y: ratio ≈ exp(-0.01 · 5) = exp(-0.05).
        let ratio = pv100 / pv0;
        assert!((ratio - (-0.01_f64 * 5.0).exp()).abs() < 1e-3);
    }

    #[test]
    fn callable_clips_holder_value() {
        let tree = TrinomialTree::build_hull_white(flat(0.02), 0.03, 0.01, 5.0, 60);
        let n = tree.steps;
        let coupon = 4.0_f64 / n as f64;
        let cf = |i: usize| {
            if i == n {
                100.0 + coupon
            } else if i > 0 {
                coupon
            } else {
                0.0
            }
        };
        let pv_uncalled = tree.price(0.0, cf, |_| None);
        let pv_called = tree.price(0.0, cf, |i| if i > 0 { Some(100.0) } else { None });
        assert!(pv_called <= pv_uncalled);
    }

    #[test]
    fn coupon_on_call_date_is_received() {
        // Mid-layer coupon coincides with a binding cap of 100. Holder value
        // ≈ disc · (cap + coupon), not disc · cap (the pre-fix forfeit).
        let tree = TrinomialTree::build_hull_white(flat(0.02), 0.03, 0.01, 1.0, 50);
        let n = tree.steps;
        let mid = n / 2;
        let cf = |i: usize| match i {
            i if i == n => 105.0,
            i if i == mid => 3.0,
            _ => 0.0,
        };
        let pv_open = tree.price(0.0, cf, |_| None);
        let pv_call = tree.price(0.0, cf, |i| if i == mid { Some(100.0) } else { None });
        let receive = (-0.02_f64 * 0.5).exp() * 103.0;
        let forfeit = (-0.02_f64 * 0.5).exp() * 100.0;
        assert!(pv_call < pv_open);
        assert!((pv_call - receive).abs() < (pv_call - forfeit).abs());
    }

    #[test]
    fn event_grid_hits_mandatory_times_exactly() {
        let mandatory = vec![0.42, 1.234, 2.7];
        let grid = build_event_grid(5.0, &mandatory, 60);
        assert!((grid[0]).abs() < 1e-12);
        assert!((grid.last().copied().unwrap() - 5.0).abs() < 1e-12);
        for m in &mandatory {
            assert!(grid.iter().any(|t| (t - m).abs() < 1e-12));
        }
        for w in grid.windows(2) {
            assert!(w[1] > w[0]);
        }
        assert!(grid.len() >= 60);
    }

    #[test]
    fn non_uniform_grid_recovers_discount() {
        let zero = |t: f64| 0.03 + 0.005 * t;
        let times = build_event_grid(4.0, &[0.71, 1.55, 2.31, 3.07], 80);
        let tree = TrinomialTree::build_hull_white_on_grid(zero, 0.03, 0.008, &times);
        let n = tree.steps;
        let pv = tree.price(0.0, |i| if i == n { 100.0 } else { 0.0 }, |_| None);
        assert!((pv - 100.0_f64 * (-zero(4.0) * 4.0).exp()).abs() < 1e-4);
    }

    #[test]
    fn step_at_time_recovers_event_indices() {
        let mandatory = vec![0.5, 1.25, 2.75];
        let times = build_event_grid(3.0, &mandatory, 30);
        let tree = TrinomialTree::build_hull_white_on_grid(flat(0.04), 0.03, 0.008, &times);
        for m in &mandatory {
            assert!(tree.step_at_time(*m).is_some());
        }
        assert_eq!(tree.step_at_time(0.0), Some(0));
        assert_eq!(tree.step_at_time(3.0), Some(tree.steps));
        assert_eq!(tree.step_at_time(0.123_456_789), None);
    }
}
