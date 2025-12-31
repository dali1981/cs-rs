// SVI fitting via Gauss-Newton optimization
//
// Fits SVI parameters to market data (log_moneyness, total_variance) pairs.
// Uses a simple manual implementation to avoid complex trait bounds.

use crate::svi::{SVIError, SVIParams};

/// Configuration for SVI fitting
#[derive(Debug, Clone)]
pub struct SVIFitterConfig {
    /// Maximum iterations
    pub max_iter: usize,
    /// Convergence tolerance
    pub tolerance: f64,
    /// Initial step size for gradient descent
    pub learning_rate: f64,
}

impl Default for SVIFitterConfig {
    fn default() -> Self {
        Self {
            max_iter: 500,
            tolerance: 1e-8,
            learning_rate: 0.01,
        }
    }
}

/// SVI fitter using gradient descent optimization
pub struct SVIFitter {
    config: SVIFitterConfig,
}

impl SVIFitter {
    pub fn new() -> Self {
        Self {
            config: SVIFitterConfig::default(),
        }
    }

    pub fn with_config(config: SVIFitterConfig) -> Self {
        Self { config }
    }

    /// Fit SVI parameters to market data
    ///
    /// # Arguments
    /// * `data` - Vector of (log_moneyness, total_variance) pairs
    ///
    /// # Returns
    /// Fitted SVI parameters or error
    pub fn fit(&self, data: &[(f64, f64)]) -> Result<SVIParams, SVIError> {
        if data.len() < 5 {
            return Err(SVIError::InsufficientData(data.len()));
        }

        // Get initial guess
        let mut params = self.initial_guess(data);

        // Gradient descent optimization
        let mut prev_cost = f64::INFINITY;
        let mut lr = self.config.learning_rate;

        for iter in 0..self.config.max_iter {
            let cost = self.compute_cost(data, &params);

            // Check convergence
            if (prev_cost - cost).abs() < self.config.tolerance {
                break;
            }

            // Adaptive learning rate
            if cost > prev_cost {
                lr *= 0.5;
            } else if iter > 0 && cost < prev_cost * 0.99 {
                lr *= 1.1;
            }
            lr = lr.clamp(1e-6, 0.1);

            prev_cost = cost;

            // Compute gradient
            let grad = self.compute_gradient(data, &params);

            // Update parameters with gradient descent
            params = self.update_params(params, &grad, lr);

            // Project to valid region
            params = self.project_to_valid(params);
        }

        // Final validation
        if !params.is_valid() {
            params = self.project_to_valid(params);
            if !params.is_valid() {
                return Err(SVIError::ConstraintViolation(
                    "Could not find valid parameters".into()
                ));
            }
        }

        Ok(params)
    }

    /// Compute sum of squared residuals
    fn compute_cost(&self, data: &[(f64, f64)], params: &SVIParams) -> f64 {
        data.iter()
            .map(|(k, w_market)| {
                let w_model = params.total_variance(*k);
                (w_market - w_model).powi(2)
            })
            .sum()
    }

    /// Compute gradient via finite differences
    fn compute_gradient(&self, data: &[(f64, f64)], params: &SVIParams) -> [f64; 5] {
        let eps = 1e-6;
        let f0 = self.compute_cost(data, params);

        let mut grad = [0.0; 5];

        // Partial derivative w.r.t. a
        let params_a = SVIParams::new(params.a + eps, params.b, params.rho, params.m, params.sigma);
        grad[0] = (self.compute_cost(data, &params_a) - f0) / eps;

        // Partial derivative w.r.t. b
        let params_b = SVIParams::new(params.a, params.b + eps, params.rho, params.m, params.sigma);
        grad[1] = (self.compute_cost(data, &params_b) - f0) / eps;

        // Partial derivative w.r.t. rho
        let params_rho = SVIParams::new(params.a, params.b, params.rho + eps, params.m, params.sigma);
        grad[2] = (self.compute_cost(data, &params_rho) - f0) / eps;

        // Partial derivative w.r.t. m
        let params_m = SVIParams::new(params.a, params.b, params.rho, params.m + eps, params.sigma);
        grad[3] = (self.compute_cost(data, &params_m) - f0) / eps;

        // Partial derivative w.r.t. sigma
        let params_sigma = SVIParams::new(params.a, params.b, params.rho, params.m, params.sigma + eps);
        grad[4] = (self.compute_cost(data, &params_sigma) - f0) / eps;

        grad
    }

    /// Update parameters using gradient descent
    fn update_params(&self, params: SVIParams, grad: &[f64; 5], lr: f64) -> SVIParams {
        SVIParams::new(
            params.a - lr * grad[0],
            params.b - lr * grad[1],
            params.rho - lr * grad[2] * 0.1, // Smaller step for rho
            params.m - lr * grad[3],
            params.sigma - lr * grad[4],
        )
    }

    /// Generate initial guess from data
    fn initial_guess(&self, data: &[(f64, f64)]) -> SVIParams {
        // Find ATM variance (closest to k=0)
        let atm_var = data
            .iter()
            .min_by(|(k1, _), (k2, _)| {
                k1.abs().partial_cmp(&k2.abs()).unwrap()
            })
            .map(|(_, v)| *v)
            .unwrap_or(0.04);

        // Estimate slope from wings
        let left_wing: Vec<_> = data.iter().filter(|(k, _)| *k < -0.1).collect();
        let right_wing: Vec<_> = data.iter().filter(|(k, _)| *k > 0.1).collect();

        let b_estimate = if !left_wing.is_empty() && !right_wing.is_empty() {
            let left_avg: f64 = left_wing.iter().map(|(_, v)| v).sum::<f64>() / left_wing.len() as f64;
            let right_avg: f64 = right_wing.iter().map(|(_, v)| v).sum::<f64>() / right_wing.len() as f64;
            ((left_avg - atm_var).abs() + (right_avg - atm_var).abs()) / 4.0
        } else {
            0.05
        };

        // Estimate skew from asymmetry
        let rho_estimate: f64 = if !left_wing.is_empty() && !right_wing.is_empty() {
            let left_avg: f64 = left_wing.iter().map(|(_, v)| v).sum::<f64>() / left_wing.len() as f64;
            let right_avg: f64 = right_wing.iter().map(|(_, v)| v).sum::<f64>() / right_wing.len() as f64;
            if left_avg > right_avg {
                -0.4 // Typical equity skew
            } else {
                0.0
            }
        } else {
            -0.3
        };

        SVIParams {
            a: atm_var * 0.8,
            b: b_estimate.max(0.01),
            rho: rho_estimate.clamp(-0.9, 0.9),
            m: 0.0,
            sigma: 0.1,
        }
    }

    /// Project parameters to valid region
    fn project_to_valid(&self, mut params: SVIParams) -> SVIParams {
        // Ensure b >= 0
        params.b = params.b.max(0.001);

        // Ensure |rho| < 1
        params.rho = params.rho.clamp(-0.99, 0.99);

        // Ensure sigma > 0
        params.sigma = params.sigma.max(0.01);

        // Ensure non-negative variance at wings
        let min_a = -params.b * params.sigma * (1.0 - params.rho.powi(2)).sqrt();
        params.a = params.a.max(min_a + 0.001);

        params
    }
}

impl Default for SVIFitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn generate_svi_data(params: &SVIParams, k_range: (f64, f64), n_points: usize) -> Vec<(f64, f64)> {
        let step = (k_range.1 - k_range.0) / (n_points - 1) as f64;
        (0..n_points)
            .map(|i| {
                let k = k_range.0 + i as f64 * step;
                let w = params.total_variance(k);
                (k, w)
            })
            .collect()
    }

    #[test]
    fn test_svi_fitter_recovers_params() {
        let true_params = SVIParams::new(0.04, 0.15, -0.4, 0.02, 0.12);
        let data = generate_svi_data(&true_params, (-0.5, 0.5), 21);

        let fitter = SVIFitter::new();
        let fitted = fitter.fit(&data).expect("Fitting should succeed");

        // Check fitted params are close to true params
        assert!(fitted.is_valid(), "Fitted params should be valid");

        // Check variance at various points matches
        for k in [-0.3, 0.0, 0.3] {
            let true_var = true_params.total_variance(k);
            let fitted_var = fitted.total_variance(k);
            assert_relative_eq!(true_var, fitted_var, epsilon = 0.02);
        }
    }

    #[test]
    fn test_svi_fitter_insufficient_data() {
        let data = vec![(0.0, 0.04), (0.1, 0.05)];
        let fitter = SVIFitter::new();
        let result = fitter.fit(&data);
        assert!(matches!(result, Err(SVIError::InsufficientData(_))));
    }

    #[test]
    fn test_svi_fitter_produces_valid_params() {
        // Realistic equity data with skew
        let data = vec![
            (-0.4, 0.12),
            (-0.3, 0.09),
            (-0.2, 0.07),
            (-0.1, 0.055),
            (0.0, 0.05),
            (0.1, 0.052),
            (0.2, 0.058),
            (0.3, 0.07),
            (0.4, 0.085),
        ];

        let fitter = SVIFitter::new();
        let fitted = fitter.fit(&data).expect("Fitting should succeed");

        assert!(fitted.is_valid(), "Fitted params should satisfy constraints");
    }

    #[test]
    fn test_initial_guess_reasonable() {
        let fitter = SVIFitter::new();

        let data = vec![
            (-0.3, 0.08),
            (-0.1, 0.05),
            (0.0, 0.04),
            (0.1, 0.045),
            (0.3, 0.06),
        ];

        let guess = fitter.initial_guess(&data);

        // Should be in reasonable ranges
        assert!(guess.a > 0.0 && guess.a < 0.5);
        assert!(guess.b > 0.0 && guess.b < 1.0);
        assert!(guess.rho > -1.0 && guess.rho < 1.0);
        assert!(guess.sigma > 0.0);
    }

    #[test]
    fn test_project_to_valid() {
        let fitter = SVIFitter::new();

        // Invalid params
        let invalid = SVIParams::new(-0.1, -0.05, 1.5, 0.0, -0.1);
        assert!(!invalid.is_valid());

        let projected = fitter.project_to_valid(invalid);
        assert!(projected.is_valid());
    }
}
