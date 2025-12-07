//! Binomial tree for interest rate modeling.
//!
//! Provides a recombining binomial tree structure for pricing bonds with
//! embedded options using backward induction.

/// A binomial interest rate tree.
///
/// The tree represents possible short rate paths over time, with each node
/// containing a short rate and transition probabilities to the next period.
///
/// # Structure
///
/// At time step `i`, there are `i + 1` possible states (nodes).
/// State `j` at time `i` is accessed via `rates[i][j]`.
///
/// ```text
///                    [0,0]
///                   /     \
///              [1,1]       [1,0]
///             /    \      /    \
///         [2,2]   [2,1]  [2,1]  [2,0]
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let tree = model.build_tree(&curve, maturity, steps);
///
/// // Get rate at time step 5, state 3
/// let rate = tree.rate_at(5, 3);
///
/// // Get discount factor with OAS spread
/// let df = tree.discount_factor(5, 3, 0.005); // 50 bps OAS
/// ```
#[derive(Debug, Clone)]
pub struct BinomialTree {
    /// Number of time steps in the tree.
    pub steps: usize,

    /// Time step size in years.
    pub dt: f64,

    /// Short rates at each node.
    /// `rates[i][j]` = short rate at time step `i`, state `j`.
    /// Dimensions: (steps + 1) x (step + 1).
    pub rates: Vec<Vec<f64>>,

    /// Transition probabilities at each node.
    /// `probabilities[i][j]` = (prob_up, prob_down) from node (i,j).
    /// Dimensions: steps x (step + 1).
    pub probabilities: Vec<Vec<(f64, f64)>>,
}

impl BinomialTree {
    /// Creates a new binomial tree with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `steps` - Number of time steps
    /// * `dt` - Time step size in years
    #[must_use]
    pub fn new(steps: usize, dt: f64) -> Self {
        let mut rates = Vec::with_capacity(steps + 1);
        let mut probabilities = Vec::with_capacity(steps);

        for i in 0..=steps {
            rates.push(vec![0.0; i + 1]);
        }

        for i in 0..steps {
            probabilities.push(vec![(0.5, 0.5); i + 1]);
        }

        Self {
            steps,
            dt,
            rates,
            probabilities,
        }
    }

    /// Returns the short rate at the given time step and state.
    ///
    /// # Panics
    ///
    /// Panics if `time_step > steps` or `state > time_step`.
    #[must_use]
    pub fn rate_at(&self, time_step: usize, state: usize) -> f64 {
        self.rates[time_step][state]
    }

    /// Sets the short rate at the given time step and state.
    ///
    /// # Panics
    ///
    /// Panics if `time_step > steps` or `state > time_step`.
    pub fn set_rate(&mut self, time_step: usize, state: usize, rate: f64) {
        self.rates[time_step][state] = rate;
    }

    /// Returns the discount factor from this node to the next time step.
    ///
    /// Uses the short rate plus an optional spread for OAS calculations.
    ///
    /// # Arguments
    ///
    /// * `time_step` - Current time step
    /// * `state` - Current state
    /// * `spread` - Spread to add to the short rate (for OAS)
    ///
    /// # Formula
    ///
    /// DF = exp(-(r + spread) * dt)
    #[must_use]
    pub fn discount_factor(&self, time_step: usize, state: usize, spread: f64) -> f64 {
        let rate = self.rates[time_step][state] + spread;
        (-rate * self.dt).exp()
    }

    /// Returns the number of states at the given time step.
    ///
    /// This is always `time_step + 1` for a recombining tree.
    #[must_use]
    pub fn states_at(&self, time_step: usize) -> usize {
        time_step + 1
    }

    /// Returns the probability of an up move from the given node.
    #[must_use]
    pub fn prob_up(&self, time_step: usize, state: usize) -> f64 {
        if time_step >= self.steps {
            return 0.5;
        }
        self.probabilities[time_step][state].0
    }

    /// Returns the probability of a down move from the given node.
    #[must_use]
    pub fn prob_down(&self, time_step: usize, state: usize) -> f64 {
        if time_step >= self.steps {
            return 0.5;
        }
        self.probabilities[time_step][state].1
    }

    /// Sets the transition probabilities at the given node.
    pub fn set_probabilities(&mut self, time_step: usize, state: usize, prob_up: f64, prob_down: f64) {
        if time_step < self.steps && state <= time_step {
            self.probabilities[time_step][state] = (prob_up, prob_down);
        }
    }

    /// Returns the time in years at the given time step.
    #[must_use]
    pub fn time_at_step(&self, time_step: usize) -> f64 {
        time_step as f64 * self.dt
    }

    /// Returns the total maturity in years.
    #[must_use]
    pub fn maturity(&self) -> f64 {
        self.steps as f64 * self.dt
    }

    /// Performs backward induction to calculate present value.
    ///
    /// This is the core pricing algorithm for option-embedded bonds.
    ///
    /// # Arguments
    ///
    /// * `terminal_values` - Values at maturity (face value typically)
    /// * `spread` - OAS spread to use in discounting
    ///
    /// # Returns
    ///
    /// Present value at time 0.
    #[must_use]
    pub fn backward_induction_simple(&self, terminal_value: f64, spread: f64) -> f64 {
        let n = self.steps;

        // Initialize values at maturity
        let mut values = vec![terminal_value; n + 1];

        // Work backwards through tree
        for i in (0..n).rev() {
            let mut new_values = vec![0.0; i + 1];

            for j in 0..=i {
                let df = self.discount_factor(i, j, spread);
                let p_up = self.prob_up(i, j);
                let p_down = self.prob_down(i, j);

                // Expected discounted value
                new_values[j] = df * (p_up * values[j + 1] + p_down * values[j]);
            }

            values = new_values;
        }

        values[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_creation() {
        let tree = BinomialTree::new(10, 0.5);

        assert_eq!(tree.steps, 10);
        assert!((tree.dt - 0.5).abs() < 1e-10);
        assert_eq!(tree.rates.len(), 11);
        assert_eq!(tree.probabilities.len(), 10);
    }

    #[test]
    fn test_states_at() {
        let tree = BinomialTree::new(5, 1.0);

        assert_eq!(tree.states_at(0), 1);
        assert_eq!(tree.states_at(1), 2);
        assert_eq!(tree.states_at(5), 6);
    }

    #[test]
    fn test_rate_access() {
        let mut tree = BinomialTree::new(3, 1.0);

        tree.set_rate(0, 0, 0.05);
        tree.set_rate(1, 0, 0.04);
        tree.set_rate(1, 1, 0.06);

        assert!((tree.rate_at(0, 0) - 0.05).abs() < 1e-10);
        assert!((tree.rate_at(1, 0) - 0.04).abs() < 1e-10);
        assert!((tree.rate_at(1, 1) - 0.06).abs() < 1e-10);
    }

    #[test]
    fn test_discount_factor() {
        let mut tree = BinomialTree::new(1, 1.0);
        tree.set_rate(0, 0, 0.05);

        // DF = exp(-0.05 * 1.0) ≈ 0.9512
        let df = tree.discount_factor(0, 0, 0.0);
        assert!((df - 0.9512_f64).abs() < 0.001);

        // With spread: exp(-(0.05 + 0.01) * 1.0) ≈ 0.9418
        let df_spread = tree.discount_factor(0, 0, 0.01);
        assert!((df_spread - 0.9418_f64).abs() < 0.001);
    }

    #[test]
    fn test_probabilities() {
        let mut tree = BinomialTree::new(2, 0.5);

        // Default probabilities
        assert!((tree.prob_up(0, 0) - 0.5).abs() < 1e-10);
        assert!((tree.prob_down(0, 0) - 0.5).abs() < 1e-10);

        // Set custom probabilities
        tree.set_probabilities(0, 0, 0.6, 0.4);
        assert!((tree.prob_up(0, 0) - 0.6).abs() < 1e-10);
        assert!((tree.prob_down(0, 0) - 0.4).abs() < 1e-10);
    }

    #[test]
    fn test_backward_induction_simple() {
        // Create a simple 1-step tree with flat rate
        let mut tree = BinomialTree::new(1, 1.0);
        tree.set_rate(0, 0, 0.05);
        tree.set_probabilities(0, 0, 0.5, 0.5);

        // Terminal value of 100
        let pv = tree.backward_induction_simple(100.0, 0.0);

        // Expected: 100 * exp(-0.05) ≈ 95.12
        assert!((pv - 95.12_f64).abs() < 0.5);
    }

    #[test]
    fn test_time_at_step() {
        let tree = BinomialTree::new(10, 0.25);

        assert!((tree.time_at_step(0) - 0.0).abs() < 1e-10);
        assert!((tree.time_at_step(4) - 1.0).abs() < 1e-10);
        assert!((tree.time_at_step(10) - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_maturity() {
        let tree = BinomialTree::new(20, 0.25);
        assert!((tree.maturity() - 5.0).abs() < 1e-10);
    }
}
