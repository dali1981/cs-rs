// Calendar spread opportunity detection in delta-space

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::delta_surface::DeltaVolSurface;
use crate::math_utils::linspace;
use crate::selection_model::{SelectionModel, SelectionIVProvider, StrikeSpaceSelection};

/// Calendar spread opportunity identified in delta-space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarOpportunity {
    /// Target delta for the spread
    pub target_delta: f64,
    /// Strike where IVs were compared (from selection model)
    pub strike: f64,
    /// Short leg expiration
    pub short_expiry: NaiveDate,
    /// Long leg expiration
    pub long_expiry: NaiveDate,
    /// IV of short leg
    pub short_iv: f64,
    /// IV of long leg
    pub long_iv: f64,
    /// Ratio of short IV to long IV
    pub iv_ratio: f64,
    /// Opportunity score (higher is better)
    pub score: f64,
}

/// Configuration for opportunity analysis
#[derive(Debug, Clone)]
pub struct OpportunityAnalyzerConfig {
    /// Minimum IV ratio to consider (short/long)
    pub min_iv_ratio: f64,
    /// Delta targets to scan
    pub delta_targets: Vec<f64>,
}

impl Default for OpportunityAnalyzerConfig {
    fn default() -> Self {
        Self {
            min_iv_ratio: 1.05,
            delta_targets: vec![0.25, 0.40, 0.50, 0.60, 0.75],
        }
    }
}

impl OpportunityAnalyzerConfig {
    /// Create config with custom delta range
    pub fn with_delta_range(min_ratio: f64, delta_start: f64, delta_end: f64, steps: usize) -> Self {
        Self {
            min_iv_ratio: min_ratio,
            delta_targets: linspace(delta_start, delta_end, steps),
        }
    }
}

/// Simple opportunity analyzer for M1.
///
/// Scans delta-space for calendar spread opportunities based on IV ratio.
pub struct OpportunityAnalyzer {
    config: OpportunityAnalyzerConfig,
    selection_provider: Box<dyn SelectionIVProvider>,
}

impl OpportunityAnalyzer {
    pub fn new(config: OpportunityAnalyzerConfig) -> Self {
        Self {
            config,
            selection_provider: Box::new(StrikeSpaceSelection),
        }
    }

    pub fn with_selection_model(mut self, model: SelectionModel) -> Self {
        self.selection_provider = model.to_provider();
        self
    }

    /// Find calendar opportunities across delta targets
    ///
    /// Returns opportunities sorted by score (best first).
    pub fn find_opportunities(
        &self,
        surface: &DeltaVolSurface,
        short_expiry: NaiveDate,
        long_expiry: NaiveDate,
    ) -> Vec<CalendarOpportunity> {
        let mut opportunities = Vec::new();

        for &delta in &self.config.delta_targets {
            // Delegate to selection provider - this maps delta to strike once,
            // then compares IVs at that same strike for both expirations
            let iv_pair = match self.selection_provider.get_iv_pair(
                surface,
                delta,
                short_expiry,
                long_expiry,
                true, // Assume calls for now
            ) {
                Some(pair) => pair,
                None => continue,
            };

            let ratio = iv_pair.short_iv / iv_pair.long_iv;

            if ratio >= self.config.min_iv_ratio {
                let score = self.score_opportunity(delta, ratio, iv_pair.short_iv);
                opportunities.push(CalendarOpportunity {
                    target_delta: delta,
                    strike: iv_pair.strike,
                    short_expiry,
                    long_expiry,
                    short_iv: iv_pair.short_iv,
                    long_iv: iv_pair.long_iv,
                    iv_ratio: ratio,
                    score,
                });
            }
        }

        // Sort by score descending
        opportunities.sort_by(|a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
        });

        opportunities
    }

    /// Find the best opportunity across all combinations of expiries
    pub fn find_best_opportunity(
        &self,
        surface: &DeltaVolSurface,
        expirations: &[NaiveDate],
    ) -> Option<CalendarOpportunity> {
        let mut best: Option<CalendarOpportunity> = None;

        for (i, &short_exp) in expirations.iter().enumerate() {
            for &long_exp in expirations.iter().skip(i + 1) {
                let opportunities = self.find_opportunities(surface, short_exp, long_exp);
                if let Some(opp) = opportunities.into_iter().next() {
                    if best.as_ref().map(|b| opp.score > b.score).unwrap_or(true) {
                        best = Some(opp);
                    }
                }
            }
        }

        best
    }

    /// Score an opportunity
    ///
    /// Scoring considers:
    /// - IV ratio: higher ratio = more edge
    /// - Absolute IV: higher IV = more theta
    /// - Delta proximity to ATM: closer to 0.5 = more liquid
    fn score_opportunity(&self, delta: f64, ratio: f64, short_iv: f64) -> f64 {
        // Higher IV ratio = more edge
        let ratio_score = (ratio - 1.0) * 10.0;

        // Higher absolute IV = more theta
        let iv_score = short_iv * 2.0;

        // Prefer deltas closer to ATM (more liquid)
        let liquidity_score = 1.0 - (delta - 0.5).abs() * 2.0;

        ratio_score + iv_score + liquidity_score
    }
}

impl Default for OpportunityAnalyzer {
    fn default() -> Self {
        Self::new(OpportunityAnalyzerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vol_slice::VolSlice;
    use chrono::Utc;

    fn create_test_surface() -> DeltaVolSurface {
        let now = Utc::now();
        let base_date = now.date_naive();
        let spot = 100.0;
        let rfr = 0.05;

        let mut surface = DeltaVolSurface::new(spot, now, "TEST".to_string(), rfr);

        // Earnings week: high IV (short expiry)
        let exp1 = base_date + chrono::Duration::days(7);
        let tte1 = 7.0 / 365.0;
        let slice1 = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.55),  // High IV for earnings
                (0.50, 0.50),
                (0.75, 0.48),
            ],
            spot, tte1, rfr, exp1,
        );
        surface.add_slice(slice1);

        // Post-earnings: normal IV (long expiry)
        let exp2 = base_date + chrono::Duration::days(30);
        let tte2 = 30.0 / 365.0;
        let slice2 = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.35),
                (0.50, 0.30),
                (0.75, 0.28),
            ],
            spot, tte2, rfr, exp2,
        );
        surface.add_slice(slice2);

        surface
    }

    #[test]
    fn test_opportunity_analyzer_finds_opportunities() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let analyzer = OpportunityAnalyzer::default();

        let opportunities = analyzer.find_opportunities(
            &surface,
            expirations[0], // 7-day
            expirations[1], // 30-day
        );

        // Should find opportunities since IV ratio > 1.05
        assert!(!opportunities.is_empty());

        // All opportunities should have IV ratio >= min
        for opp in &opportunities {
            assert!(opp.iv_ratio >= 1.05);
        }
    }

    #[test]
    fn test_opportunity_analyzer_sorted_by_score() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let analyzer = OpportunityAnalyzer::default();

        let opportunities = analyzer.find_opportunities(
            &surface,
            expirations[0],
            expirations[1],
        );

        // Should be sorted by score descending
        for i in 1..opportunities.len() {
            assert!(
                opportunities[i - 1].score >= opportunities[i].score,
                "Opportunities should be sorted by score descending"
            );
        }
    }

    #[test]
    fn test_opportunity_analyzer_iv_ratio() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let analyzer = OpportunityAnalyzer::default();

        let opportunities = analyzer.find_opportunities(
            &surface,
            expirations[0],
            expirations[1],
        );

        // Check IV ratio calculation
        for opp in &opportunities {
            let expected_ratio = opp.short_iv / opp.long_iv;
            assert!((opp.iv_ratio - expected_ratio).abs() < 1e-10);
        }
    }

    #[test]
    fn test_opportunity_analyzer_with_custom_config() {
        let surface = create_test_surface();
        let expirations = surface.expirations();

        // Set high min ratio that filters everything
        let config = OpportunityAnalyzerConfig {
            min_iv_ratio: 2.0,  // Very high
            delta_targets: vec![0.50],
        };
        let analyzer = OpportunityAnalyzer::new(config);

        let opportunities = analyzer.find_opportunities(
            &surface,
            expirations[0],
            expirations[1],
        );

        // Should find no opportunities with such high min ratio
        assert!(opportunities.is_empty());
    }

    #[test]
    fn test_opportunity_analyzer_delta_range() {
        let config = OpportunityAnalyzerConfig::with_delta_range(1.0, 0.20, 0.80, 7);

        assert_eq!(config.delta_targets.len(), 7);
        assert!((config.delta_targets[0] - 0.20).abs() < 1e-10);
        assert!((config.delta_targets[6] - 0.80).abs() < 1e-10);
    }

    #[test]
    fn test_opportunity_analyzer_find_best() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let analyzer = OpportunityAnalyzer::default();

        let best = analyzer.find_best_opportunity(&surface, &expirations);
        assert!(best.is_some());

        let best = best.unwrap();
        // Best should have the highest score among all opportunities
        let all_opportunities = analyzer.find_opportunities(
            &surface,
            expirations[0],
            expirations[1],
        );

        if let Some(top) = all_opportunities.first() {
            assert!((best.score - top.score).abs() < 1e-10);
        }
    }

    #[test]
    fn test_opportunity_scoring() {
        let analyzer = OpportunityAnalyzer::default();

        // Higher IV ratio should score higher (same delta and IV)
        let score1 = analyzer.score_opportunity(0.50, 1.20, 0.40);
        let score2 = analyzer.score_opportunity(0.50, 1.10, 0.40);
        assert!(score1 > score2);

        // ATM delta should score higher than wing (same ratio and IV)
        let score_atm = analyzer.score_opportunity(0.50, 1.20, 0.40);
        let score_wing = analyzer.score_opportunity(0.25, 1.20, 0.40);
        assert!(score_atm > score_wing);
    }
}
