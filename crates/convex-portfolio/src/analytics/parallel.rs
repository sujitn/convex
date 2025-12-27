//! Parallel processing utilities for portfolio analytics.
//!
//! Provides conditional parallel iteration based on configuration
//! and collection size. Uses rayon when the `parallel` feature is enabled.

use crate::types::AnalyticsConfig;

/// Maps a function over items, conditionally using parallel iteration.
///
/// Uses parallel iteration when:
/// - The `parallel` feature is enabled
/// - `config.parallel` is true
/// - The collection size exceeds `config.parallel_threshold`
///
/// # Example
///
/// ```ignore
/// let results = maybe_parallel_map(&holdings, &config, |h| h.market_value());
/// ```
#[allow(unused_variables)]
pub fn maybe_parallel_map<T, U, F>(items: &[T], config: &AnalyticsConfig, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync + Send,
{
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        if config.should_parallelize(items.len()) {
            return items.par_iter().map(f).collect();
        }
    }

    items.iter().map(f).collect()
}

/// Folds over items with a reduce step, conditionally using parallel iteration.
///
/// Uses parallel iteration when:
/// - The `parallel` feature is enabled
/// - `config.parallel` is true
/// - The collection size exceeds `config.parallel_threshold`
///
/// # Arguments
///
/// * `items` - The collection to process
/// * `config` - Analytics configuration
/// * `identity` - The identity value for the fold
/// * `fold` - The fold function: `(accumulator, item) -> accumulator`
/// * `reduce` - The reduce function: `(acc1, acc2) -> combined`
///
/// # Example
///
/// ```ignore
/// let (sum_weighted, sum_weights) = maybe_parallel_fold(
///     &holdings,
///     &config,
///     (0.0, 0.0),
///     |(sum_w, sum_wt), h| {
///         let weight = h.market_value().to_f64();
///         (sum_w + h.ytm() * weight, sum_wt + weight)
///     },
///     |(a, b), (c, d)| (a + c, b + d),
/// );
/// ```
#[allow(unused_variables)]
pub fn maybe_parallel_fold<T, U, F, R>(
    items: &[T],
    config: &AnalyticsConfig,
    identity: U,
    fold: F,
    reduce: R,
) -> U
where
    T: Sync,
    U: Send + Sync + Clone,
    F: Fn(U, &T) -> U + Sync + Send,
    R: Fn(U, U) -> U + Sync + Send,
{
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        if config.should_parallelize(items.len()) {
            return items
                .par_iter()
                .fold(|| identity.clone(), &fold)
                .reduce(|| identity.clone(), reduce);
        }
    }

    items.iter().fold(identity, fold)
}

/// Filters and maps items, conditionally using parallel iteration.
#[allow(unused_variables)]
pub fn maybe_parallel_filter_map<T, U, F>(items: &[T], config: &AnalyticsConfig, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> Option<U> + Sync + Send,
{
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        if config.should_parallelize(items.len()) {
            return items.par_iter().filter_map(f).collect();
        }
    }

    items.iter().filter_map(f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maybe_parallel_map() {
        let config = AnalyticsConfig::sequential();
        let items = vec![1, 2, 3, 4, 5];
        let results: Vec<i32> = maybe_parallel_map(&items, &config, |x| x * 2);
        assert_eq!(results, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn test_maybe_parallel_fold() {
        let config = AnalyticsConfig::sequential();
        let items: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sum: f64 = maybe_parallel_fold(&items, &config, 0.0, |acc, x| acc + x, |a, b| a + b);
        assert!((sum - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_maybe_parallel_filter_map() {
        let config = AnalyticsConfig::sequential();
        let items = vec![1, 2, 3, 4, 5];
        let results: Vec<i32> =
            maybe_parallel_filter_map(&items, &config, |x| if *x > 2 { Some(x * 2) } else { None });
        assert_eq!(results, vec![6, 8, 10]);
    }

    #[test]
    fn test_parallel_threshold() {
        // Below threshold - should use sequential
        let config = AnalyticsConfig::default().with_threshold(10);
        let small: Vec<i32> = (0..5).collect();
        assert!(!config.should_parallelize(small.len()));

        // Above threshold - would use parallel if feature enabled
        let _large: Vec<i32> = (0..100).collect();
        // Note: this only returns true if the parallel feature is enabled
        #[cfg(feature = "parallel")]
        assert!(config.should_parallelize(_large.len()));
    }
}
