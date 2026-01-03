/// Calculate IV percentile over lookback period
///
/// Returns percentage of historical IVs that are below current IV.
///
/// # Example
/// ```
/// use cs_analytics::iv_percentile;
///
/// let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
/// let percentile = iv_percentile(0.40, &historical);
/// assert_eq!(percentile, 60.0); // 3 out of 5 are below 0.40 = 60th percentile
/// ```
pub fn iv_percentile(current_iv: f64, historical_ivs: &[f64]) -> f64 {
    if historical_ivs.is_empty() {
        return 50.0;
    }

    let count_below = historical_ivs.iter().filter(|&&iv| iv < current_iv).count();
    (count_below as f64 / historical_ivs.len() as f64) * 100.0
}

/// Calculate IV rank (position in range)
///
/// Returns (current - min) / (max - min) as percentage.
///
/// # Example
/// ```
/// use cs_analytics::iv_rank;
///
/// let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
/// let rank = iv_rank(0.30, &historical);
/// assert_eq!(rank, 50.0); // At midpoint of range [0.10, 0.50]
/// ```
pub fn iv_rank(current_iv: f64, historical_ivs: &[f64]) -> f64 {
    if historical_ivs.is_empty() {
        return 50.0;
    }

    let min = historical_ivs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = historical_ivs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    if (max - min).abs() < 1e-10 {
        return 50.0;
    }

    ((current_iv - min) / (max - min)) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_iv_percentile_empty() {
        let percentile = iv_percentile(0.30, &[]);
        assert_eq!(percentile, 50.0);
    }

    #[test]
    fn test_iv_percentile_median() {
        let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
        let percentile = iv_percentile(0.30, &historical);
        assert_eq!(percentile, 40.0); // 2 out of 5 are below 0.30
    }

    #[test]
    fn test_iv_percentile_min() {
        let historical = vec![0.20, 0.30, 0.40, 0.50];
        let percentile = iv_percentile(0.10, &historical);
        assert_eq!(percentile, 0.0); // All are above
    }

    #[test]
    fn test_iv_percentile_max() {
        let historical = vec![0.10, 0.20, 0.30, 0.40];
        let percentile = iv_percentile(0.50, &historical);
        assert_eq!(percentile, 100.0); // All are below
    }

    #[test]
    fn test_iv_rank_empty() {
        let rank = iv_rank(0.30, &[]);
        assert_eq!(rank, 50.0);
    }

    #[test]
    fn test_iv_rank_constant() {
        let historical = vec![0.30, 0.30, 0.30, 0.30];
        let rank = iv_rank(0.30, &historical);
        assert_eq!(rank, 50.0); // No range
    }

    #[test]
    fn test_iv_rank_min() {
        let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
        let rank = iv_rank(0.10, &historical);
        assert_eq!(rank, 0.0); // At minimum
    }

    #[test]
    fn test_iv_rank_max() {
        let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
        let rank = iv_rank(0.50, &historical);
        assert_eq!(rank, 100.0); // At maximum
    }

    #[test]
    fn test_iv_rank_mid() {
        let historical = vec![0.10, 0.20, 0.30, 0.40, 0.50];
        let rank = iv_rank(0.30, &historical);
        assert_relative_eq!(rank, 50.0, epsilon = 1e-10); // In middle of range
    }
}
