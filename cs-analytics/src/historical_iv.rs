/// Calculate IV percentile over lookback period
pub fn iv_percentile(current_iv: f64, historical_ivs: &[f64]) -> f64 {
    if historical_ivs.is_empty() {
        return 50.0;
    }

    let count_below = historical_ivs.iter().filter(|&&iv| iv < current_iv).count();
    (count_below as f64 / historical_ivs.len() as f64) * 100.0
}

/// Calculate IV rank (position in range)
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

/// Calculate realized volatility from price returns
pub fn realized_volatility(
    prices: &[f64],
    window: usize,
    annualization_factor: f64,
) -> Option<f64> {
    if prices.len() < window + 1 {
        return None;
    }

    // Calculate log returns
    let returns: Vec<f64> = prices.windows(2)
        .map(|w| (w[1] / w[0]).ln())
        .collect();

    if returns.len() < window {
        return None;
    }

    // Take last `window` returns
    let recent_returns = &returns[returns.len() - window..];

    // Calculate standard deviation
    let mean = recent_returns.iter().sum::<f64>() / window as f64;
    let variance = recent_returns.iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / (window - 1) as f64;

    Some(variance.sqrt() * annualization_factor.sqrt())
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

    #[test]
    fn test_realized_volatility_insufficient_data() {
        let prices = vec![100.0, 101.0];
        let vol = realized_volatility(&prices, 10, 252.0);
        assert!(vol.is_none());
    }

    #[test]
    fn test_realized_volatility_stable_prices() {
        let prices = vec![100.0; 30];
        let vol = realized_volatility(&prices, 20, 252.0);

        assert!(vol.is_some());
        // Volatility should be very close to zero for constant prices
        assert_relative_eq!(vol.unwrap(), 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_realized_volatility_calculation() {
        // Simple test with known values
        let prices = vec![100.0, 101.0, 102.0, 101.5, 103.0, 102.5];
        let vol = realized_volatility(&prices, 5, 252.0);

        assert!(vol.is_some());
        let vol_value = vol.unwrap();

        // Volatility should be positive
        assert!(vol_value > 0.0);

        // For these moderate moves, should be reasonable (not too high/low)
        assert!(vol_value < 1.0); // Less than 100% annualized
    }

    #[test]
    fn test_realized_volatility_exact_window() {
        let prices = vec![100.0, 101.0, 102.0, 103.0, 104.0];
        let vol = realized_volatility(&prices, 4, 252.0);

        assert!(vol.is_some());
    }

    #[test]
    fn test_realized_volatility_annualization() {
        let prices: Vec<f64> = (0..100).map(|i| 100.0 + (i as f64 * 0.1)).collect();

        let vol_daily = realized_volatility(&prices, 20, 252.0);
        let vol_weekly = realized_volatility(&prices, 20, 52.0);

        assert!(vol_daily.is_some());
        assert!(vol_weekly.is_some());

        // Higher annualization factor should give higher volatility
        assert!(vol_daily.unwrap() > vol_weekly.unwrap());
    }
}
