//! Bounded recombining OU lattice, with exact supplied curve state-price fit.

use super::{b, finite, invalid, positive, Hw1fError, Hw1fParameters};

/// Maximum event-aligned intervals, after subdivision.
pub const MAX_TREE_STEPS: usize = 2048;
/// Maximum nodes over all rows of one tree; checked before row allocation.
pub const MAX_TREE_NODES: usize = 2_000_000;
const MAX_ROW_NODES: usize = 8193;

/// Every supplied event is retained exactly; no date is moved to a nearby layer.
pub fn event_grid(events: &[f64], desired_steps: usize) -> Result<Vec<f64>, Hw1fError> {
    if events.is_empty()
        || events.len() > MAX_TREE_STEPS
        || desired_steps == 0
        || desired_steps > MAX_TREE_STEPS
    {
        return Err(Hw1fError::Bound("event grid dimensions"));
    }
    if events
        .iter()
        .any(|t| !t.is_finite() || *t < 0.0 || *t > 150.0)
    {
        return Err(invalid("event times", "requires finite times in [0,150]"));
    }
    let mut anchors = events.to_vec();
    anchors.push(0.0);
    anchors.sort_by(f64::total_cmp);
    anchors.dedup();
    let end = *anchors.last().ok_or_else(|| invalid("events", "empty"))?;
    if end <= 0.0 {
        return Err(invalid("events", "requires a positive horizon"));
    }
    let target = end / desired_steps as f64;
    let mut times = vec![0.0];
    for pair in anchors.windows(2) {
        let span = pair[1] - pair[0];
        let count = (span / target).ceil().max(1.0) as usize;
        if times.len() + count > MAX_TREE_STEPS + 1 {
            return Err(Hw1fError::Bound("event-aligned intervals"));
        }
        for index in 1..count {
            times.push(pair[0] + span * index as f64 / count as f64);
        }
        times.push(pair[1]);
    }
    Ok(times)
}

#[derive(Debug, Clone)]
struct Transition {
    children: [usize; 3],
    probabilities: [f64; 3],
}
#[derive(Debug, Clone)]
struct Layer {
    minimum: i32,
    width: usize,
    spacing: f64,
    shift: f64,
    transitions: Vec<Transition>,
}

/// Actual numerical curve-fit/resource information; not a convergence certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeDiagnostics {
    /// Actual event-aligned intervals.
    pub steps: usize,
    /// Total allocated lattice nodes.
    pub nodes: usize,
    /// Largest state-price discount-factor residual on the grid.
    pub maximum_discount_residual: f64,
    /// Smallest transition probability (no probability is clipped).
    pub minimum_probability: f64,
}

/// Immutable validated one-factor tree. All time-zero DFs are relative to its root.
#[derive(Debug, Clone)]
pub struct Hw1fTree {
    times: Vec<f64>,
    layers: Vec<Layer>,
    diagnostics: TreeDiagnostics,
}
impl Hw1fTree {
    /// Build with actual grid DFs. The origin must be exactly time 0 with DF 1.
    #[allow(clippy::manual_midpoint)] // Moment probability polynomials; |error| <= 1/2.
    pub fn build(
        parameters: Hw1fParameters,
        times: &[f64],
        discounts: &[f64],
    ) -> Result<Self, Hw1fError> {
        parameters.validate()?;
        if times.len() < 2 || times.len() > MAX_TREE_STEPS + 1 || discounts.len() != times.len() {
            return Err(Hw1fError::Bound("tree dimensions"));
        }
        if times[0] != 0.0
            || discounts[0] != 1.0
            || times
                .iter()
                .any(|t| !t.is_finite() || *t < 0.0 || *t > 150.0)
            || times.windows(2).any(|p| p[1] <= p[0])
            || discounts.iter().any(|d| !d.is_finite() || *d <= 0.0)
        {
            return Err(invalid(
                "tree inputs",
                "strict times, positive finite root-normalized DFs required",
            ));
        }
        let mut layers = vec![Layer {
            minimum: 0,
            width: 1,
            spacing: 0.0,
            shift: 0.0,
            transitions: vec![],
        }];
        let mut prices = vec![1.0];
        let mut diagnostics = TreeDiagnostics {
            steps: times.len() - 1,
            nodes: 1,
            maximum_discount_residual: 0.0,
            minimum_probability: 1.0,
        };
        for i in 0..times.len() - 1 {
            let dt = times[i + 1] - times[i];
            let variance = finite(
                parameters.volatility.powi(2) * b(2.0 * parameters.mean_reversion, dt),
                "OU variance",
            )?;
            let spacing = (3.0 * variance).sqrt();
            let mean_factor = (-parameters.mean_reversion * dt).exp();
            let current = &layers[i];
            let mut provisional = Vec::with_capacity(current.width);
            let mut next_min = i32::MAX;
            let mut next_max = i32::MIN;
            let mut numerator = 0.0;
            for (index, state_price) in prices.iter().enumerate() {
                let x = (current.minimum + index as i32) as f64 * current.spacing;
                numerator += *state_price * positive((-x * dt).exp(), "state-price exponential")?;
                let (center, probabilities) = if parameters.volatility == 0.0 {
                    (0, [0.0, 1.0, 0.0])
                } else {
                    positive(spacing, "OU spacing")?;
                    let target = finite(x * mean_factor / spacing, "OU conditional mean")?;
                    if target.abs() > MAX_ROW_NODES as f64 {
                        return Err(Hw1fError::Bound("OU child state"));
                    }
                    let center = target.round() as i32;
                    let error = target - center as f64;
                    (
                        center,
                        [
                            1.0 / 6.0 + (error * error - error) / 2.0,
                            2.0 / 3.0 - error * error,
                            1.0 / 6.0 + (error * error + error) / 2.0,
                        ],
                    )
                };
                for probability in probabilities {
                    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
                        return Err(Hw1fError::Numerical("OU transition probability"));
                    }
                    diagnostics.minimum_probability =
                        diagnostics.minimum_probability.min(probability);
                }
                if (probabilities.iter().sum::<f64>() - 1.0).abs() > 1e-14 {
                    return Err(Hw1fError::Numerical("probability conservation"));
                }
                let reach = i32::from(parameters.volatility != 0.0);
                next_min = next_min.min(center - reach);
                next_max = next_max.max(center + reach);
                provisional.push((center, probabilities));
            }
            let next_width = (next_max - next_min + 1) as usize;
            if next_width > MAX_ROW_NODES || diagnostics.nodes + next_width > MAX_TREE_NODES {
                return Err(Hw1fError::Bound("tree nodes"));
            }
            diagnostics.nodes += next_width;
            let shift = finite(
                (positive(numerator, "curve fit numerator")? / discounts[i + 1]).ln() / dt,
                "curve fitting shift",
            )?;
            let mut next_prices = vec![0.0; next_width];
            let mut transitions = Vec::with_capacity(current.width);
            for (index, (center, probabilities)) in provisional.into_iter().enumerate() {
                let x = (current.minimum + index as i32) as f64 * current.spacing;
                let weight = finite(
                    prices[index] * positive((-(shift + x) * dt).exp(), "fitted discount")?,
                    "state price",
                )?;
                let middle = (center - next_min) as usize;
                let children = if parameters.volatility == 0.0 {
                    [0, 0, 0]
                } else {
                    [middle - 1, middle, middle + 1]
                };
                for (child, probability) in children.iter().zip(probabilities) {
                    next_prices[*child] = finite(
                        next_prices[*child] + weight * probability,
                        "state-price propagation",
                    )?;
                }
                transitions.push(Transition {
                    children,
                    probabilities,
                });
            }
            let fitted: f64 = next_prices.iter().sum();
            let residual = finite((fitted - discounts[i + 1]).abs(), "curve-fit residual")?;
            if residual > 2e-12 + 2e-12 * discounts[i + 1].abs() {
                return Err(Hw1fError::Numerical("initial curve fit postcondition"));
            }
            diagnostics.maximum_discount_residual =
                diagnostics.maximum_discount_residual.max(residual);
            layers[i].shift = shift;
            layers[i].transitions = transitions;
            layers.push(Layer {
                minimum: next_min,
                width: next_width,
                spacing,
                shift: 0.0,
                transitions: vec![],
            });
            prices = next_prices;
        }
        Ok(Self {
            times: times.to_vec(),
            layers,
            diagnostics,
        })
    }
    /// Actual grid times, in ACT/365F years for dated composition.
    pub fn times(&self) -> &[f64] {
        &self.times
    }
    /// Curve fit and exact resource dimensions.
    pub fn diagnostics(&self) -> &TreeDiagnostics {
        &self.diagnostics
    }
    /// Exact lookup, not a nearest event bucket.
    pub fn layer_at(&self, time: f64) -> Result<usize, Hw1fError> {
        self.times
            .binary_search_by(|t| t.total_cmp(&time))
            .map_err(|_| invalid("time", "not an exact grid event"))
    }
    /// Validated row width.
    pub fn width(&self, layer: usize) -> Result<usize, Hw1fError> {
        self.layers
            .get(layer)
            .map(|row| row.width)
            .ok_or_else(|| invalid("layer", "outside grid"))
    }
    /// Short-rate state spacing in decimal-rate units. A price-converged tree
    /// can still undersample a small option-risk bump.
    pub fn state_spacing(&self, layer: usize) -> Result<f64, Hw1fError> {
        self.layers
            .get(layer)
            .map(|row| row.spacing)
            .ok_or_else(|| invalid("layer", "outside grid"))
    }
    /// Roll a next-row value back one interval, adding OAS only to discounting.
    pub fn rollback(&self, layer: usize, values: &[f64], oas: f64) -> Result<Vec<f64>, Hw1fError> {
        let row = self
            .layers
            .get(layer)
            .ok_or_else(|| invalid("layer", "outside grid"))?;
        let next = self
            .layers
            .get(layer + 1)
            .ok_or_else(|| invalid("layer", "no following interval"))?;
        if !oas.is_finite()
            || oas.abs() > 1.0
            || values.len() != next.width
            || values.iter().any(|v| !v.is_finite())
        {
            return Err(invalid("rollback", "invalid spread, dimensions or values"));
        }
        let dt = self.times[layer + 1] - self.times[layer];
        row.transitions
            .iter()
            .enumerate()
            .map(|(index, transition)| {
                let expectation: f64 = transition
                    .children
                    .iter()
                    .zip(transition.probabilities)
                    .map(|(child, probability)| values[*child] * probability)
                    .sum();
                let x = (row.minimum + index as i32) as f64 * row.spacing;
                finite(
                    expectation
                        * positive((-(row.shift + x + oas) * dt).exp(), "rollback discount")?,
                    "rollback value",
                )
            })
            .collect()
    }
    /// Conditional value of deterministic cash at an exact decision layer.
    /// Entries may share a layer but cannot precede the decision or leave the grid.
    pub fn cash_value_at(
        &self,
        decision: usize,
        cash: &[(usize, f64)],
        oas: f64,
    ) -> Result<Vec<f64>, Hw1fError> {
        self.width(decision)?;
        if cash.len() > MAX_TREE_STEPS || !oas.is_finite() || oas.abs() > 1.0 {
            return Err(Hw1fError::Bound("cash rollback inputs"));
        }
        let mut amounts = vec![0.0; self.times.len()];
        let mut end = decision;
        for &(layer, amount) in cash {
            if layer < decision || layer >= amounts.len() || !amount.is_finite() {
                return Err(invalid("cash", "invalid layer/amount"));
            }
            amounts[layer] = finite(amounts[layer] + amount, "cash sum")?;
            end = end.max(layer);
        }
        let mut values = vec![amounts[end]; self.width(end)?];
        for layer in (decision..end).rev() {
            values = self.rollback(layer, &values, oas)?;
            for value in &mut values {
                *value = finite(*value + amounts[layer], "cash value")?;
            }
        }
        Ok(values)
    }
}
