//! Hagan-Brace trinomial tree for the Hull-White 1F short-rate model.
//! Matches QL's `TreeCallableFixedRateBondEngine` lattice. See Brigo–Mercurio
//! §3.7 or Hull §31.7.

/// Trinomial branching from node (i, j): central child index `k` and the
/// up/mid/down probabilities (sum to 1).
#[derive(Debug, Clone, Copy)]
struct Branching {
    k: i32,
    pu: f64,
    pm: f64,
    pd: f64,
}

/// Pure function of `(j, j_max, m)` where `m = exp(-a·Δt)`. Probabilities
/// from the standard moment-matching derivation; the boundary branches at
/// `±j_max` use the alternative {k+1, k, k-1} = {j_max, j_max-1, j_max-2}
/// (top) or {-j_max+2, -j_max+1, -j_max} (bottom) layout.
fn branching_at(j: i32, j_max: i32, m: f64) -> Branching {
    let target = j as f64 * m;
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

#[derive(Debug, Clone)]
pub struct TrinomialTree {
    pub steps: usize,
    pub dt: f64,
    pub dx: f64,
    pub j_max: i32,
    /// Mean-reversion scaling `m = exp(-a·Δt)`.
    m: f64,
    /// `alpha[i]` shifts the short rate at step i so the tree exactly
    /// reprices `D(t_{i+1})`.
    pub alpha: Vec<f64>,
}

impl TrinomialTree {
    /// Build the HW1F tree fitting `zero_rates(t)` (continuously compounded).
    /// `t_i = i·Δt` where `Δt = maturity / steps`.
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
        assert!(a > 0.0, "Hull-White mean reversion must be positive");
        assert!(sigma > 0.0, "Hull-White volatility must be positive");
        assert!(steps > 0, "trinomial tree needs at least 1 step");

        let dt = maturity / steps as f64;
        let dx = sigma * (3.0 * dt).sqrt();
        let j_max = ((0.184 / (a * dt)).ceil().max(1.0)) as i32;
        let m = (-a * dt).exp();

        // Forward induction with Arrow-Debreu prices Q[i][j+j_max].
        let row_len = (2 * j_max + 1) as usize;
        let mut alpha = vec![0.0f64; steps + 1];
        let mut q = vec![0.0f64; row_len];
        q[j_max as usize] = 1.0;
        let mut q_next = vec![0.0f64; row_len];

        for (i, alpha_i) in alpha.iter_mut().enumerate().take(steps) {
            let band = (i as i32).min(j_max);

            // exp(-α[i]·dt) · Σ_j Q[i][j] · exp(-x_j·dt) = D(t_{i+1})
            let mut numer = 0.0f64;
            for j in -band..=band {
                let qij = q[(j + j_max) as usize];
                if qij > 0.0 {
                    numer += qij * (-(j as f64 * dx) * dt).exp();
                }
            }
            let d_next = (-zero_rates((i + 1) as f64 * dt) * (i + 1) as f64 * dt).exp();
            *alpha_i = (numer / d_next).max(1e-300).ln() / dt;

            // Propagate Q[i] → Q[i+1].
            q_next.fill(0.0);
            for j in -band..=band {
                let qij = q[(j + j_max) as usize];
                if qij == 0.0 {
                    continue;
                }
                let br = branching_at(j, j_max, m);
                let weight = qij * (-(*alpha_i + j as f64 * dx) * dt).exp();
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
            dt,
            dx,
            j_max,
            m,
            alpha,
        }
    }

    /// Backward-induction PV of a stream of cashflows.
    ///
    /// `cashflow_at(i)` is added at every reachable node at step i (zero
    /// except on coupon and maturity steps). `call_price_at(i)` returns the
    /// *dirty* call cap (clean price + accrued at the exercise date) when the
    /// bond is callable on that step's date; the holder value is then
    /// `min(value, cap)`. `oas` is added to the short rate before discounting.
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
            new_values.fill(0.0);
            for j in -band..=band {
                let br = branching_at(j, self.j_max, self.m);
                let r = self.alpha[i] + j as f64 * self.dx + oas;
                let df = (-r * self.dt).exp();
                let v_up = values[(br.k + 1 + self.j_max) as usize];
                let v_mid = values[(br.k + self.j_max) as usize];
                let v_down = values[(br.k - 1 + self.j_max) as usize];
                let cont = df * (br.pu * v_up + br.pm * v_mid + br.pd * v_down);
                let after_coupon = cont + cashflow_at(i);
                let after_call = match call_price_at(i) {
                    // QL convention: callability replaces the bond's
                    // principal; today's coupon flows to the holder via
                    // cashflow_at(i), but the cap is the call price alone.
                    Some(cap) if i > 0 => after_coupon.min(cap),
                    _ => after_coupon,
                };
                new_values[(j + self.j_max) as usize] = after_call;
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
        let t_half = tree.dt * half as f64;
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
        assert!(pv_called <= 100.0 + 1e-9);
    }
}
