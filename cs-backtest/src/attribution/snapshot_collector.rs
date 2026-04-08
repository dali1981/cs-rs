//! Position snapshot collection for P&L attribution
//!
//! Collects daily (open, close) snapshots during the holding period,
//! recomputing Greeks from IV surfaces or volatility values as configured.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use std::sync::Arc;

use cs_domain::{
    AttributionConfig, CompositeTrade, EquityDataRepository, HedgeAction,
    OptionsDataRepository, PositionAttribution, PositionGreeks, PositionSnapshot,
    SnapshotTimes, TradingCalendar, VolatilitySource,
};
use cs_analytics::{realized_volatility, PricingModel};
use rust_decimal::Decimal;

use super::greeks_computer::GreeksComputer;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Collects position snapshots for P&L attribution
///
/// Workflow:
/// 1. Created with trade, repos, and config
/// 2. set_hedge_timeline() called after hedging completes
/// 3. collect() builds daily snapshots
/// 4. build_attribution() creates final PositionAttribution
pub struct SnapshotCollector<T: CompositeTrade + Clone> {
    trade: T,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    config: AttributionConfig,
    contract_multiplier: i32,
    risk_free_rate: f64,

    /// Collected snapshot pairs: (open, close) for each trading day
    snapshots: Vec<(PositionSnapshot, PositionSnapshot)>,

    /// Hedge shares timeline from HedgePosition
    /// Populated after hedging completes
    hedge_timeline: Vec<(DateTime<Utc>, i32)>,

    /// Entry volatility (for EntryIV/EntryHV modes)
    entry_vol: Option<f64>,
}

impl<T: CompositeTrade + Clone> SnapshotCollector<T> {
    pub fn new(
        trade: T,
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        symbol: String,
        config: AttributionConfig,
        contract_multiplier: i32,
        risk_free_rate: f64,
    ) -> Self {
        Self {
            trade,
            options_repo,
            equity_repo,
            symbol,
            config,
            contract_multiplier,
            risk_free_rate,
            snapshots: Vec::new(),
            hedge_timeline: Vec::new(),
            entry_vol: None,
        }
    }

    /// Set entry volatility (for EntryIV/EntryHV modes)
    pub fn set_entry_vol(&mut self, vol: f64) {
        self.entry_vol = Some(vol);
    }

    /// Set hedge timeline after hedging phase completes
    ///
    /// Converts HedgeActions into cumulative share timeline
    pub fn set_hedge_timeline(&mut self, hedges: &[HedgeAction]) {
        let mut cumulative = 0i32;
        self.hedge_timeline.clear();

        for hedge in hedges {
            cumulative += hedge.shares;
            self.hedge_timeline.push((hedge.timestamp, cumulative));
        }
    }

    /// Get hedge shares at a specific time
    ///
    /// Returns most recent hedge position before or at timestamp
    fn hedge_shares_at(&self, timestamp: DateTime<Utc>) -> i32 {
        self.hedge_timeline
            .iter()
            .rev()
            .find(|(t, _)| *t <= timestamp)
            .map(|(_, shares)| *shares)
            .unwrap_or(0)
    }

    /// Collect snapshots for all trading days in range
    ///
    /// Creates (open, close) snapshot pairs for each day, skipping days
    /// with missing market data.
    pub async fn collect(
        &mut self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Result<(), String> {
        let start_date = entry_time.date_naive();
        let end_date = exit_time.date_naive();

        let trading_days: Vec<NaiveDate> =
            TradingCalendar::trading_days_between(start_date, end_date).collect();

        for date in trading_days {
            match self.collect_day(date).await {
                Ok(Some((open, close))) => {
                    self.snapshots.push((open, close));
                }
                Ok(None) => {
                    // Skip days with missing data
                    tracing::debug!("Skipping attribution for {}: missing data", date);
                }
                Err(e) => {
                    tracing::warn!("Attribution snapshot failed for {}: {}", date, e);
                }
            }
        }

        Ok(())
    }

    /// Collect (open, close) snapshots for a single day
    async fn collect_day(
        &self,
        date: NaiveDate,
    ) -> Result<Option<(PositionSnapshot, PositionSnapshot)>, String> {
        let (open_time, close_time) = self.snapshot_times(date);

        // For CloseOnly mode, don't duplicate work
        if matches!(self.config.snapshot_times, SnapshotTimes::CloseOnly) {
            let close_snapshot = self.create_snapshot(close_time).await?;
            // Use same snapshot for both open and close
            return Ok(Some((close_snapshot.clone(), close_snapshot)));
        }

        // Collect open snapshot
        let open_snapshot = match self.create_snapshot(open_time).await {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        // Collect close snapshot
        let close_snapshot = match self.create_snapshot(close_time).await {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        Ok(Some((open_snapshot, close_snapshot)))
    }

    /// Create a single snapshot at a specific time
    async fn create_snapshot(&self, timestamp: DateTime<Utc>) -> Result<PositionSnapshot, String> {
        // 1. Get spot price
        let spot = self
            .equity_repo
            .get_spot_price(&self.symbol, timestamp)
            .await
            .map_err(|e| e.to_string())?
            .to_f64();

        // 2. Get volatility and compute Greeks based on configured source
        let (iv, greeks) = match &self.config.vol_source {
            VolatilitySource::EntryIV => {
                let vol = self
                    .entry_vol
                    .ok_or("EntryIV mode requires set_entry_vol() to be called")?;
                self.compute_with_entry_vol(spot, vol, timestamp)?
            }
            VolatilitySource::EntryHV { window: _ } => {
                let vol = self
                    .entry_vol
                    .ok_or("EntryHV mode requires set_entry_vol() to be called")?;
                self.compute_with_entry_vol(spot, vol, timestamp)?
            }
            VolatilitySource::CurrentMarketIV => {
                self.compute_with_current_market_iv(spot, timestamp).await?
            }
            VolatilitySource::CurrentHV { window } => {
                self.compute_with_current_hv(spot, timestamp, *window)
                    .await?
            }
            VolatilitySource::HistoricalAverageIV { lookback_days } => {
                return Err(format!(
                    "HistoricalAverageIV not yet supported for attribution (lookback: {})",
                    lookback_days
                ));
            }
        };

        // 3. Get hedge shares at this time
        let hedge_shares = self.hedge_shares_at(timestamp);

        Ok(PositionSnapshot::new(
            timestamp,
            spot,
            iv,
            greeks,
            hedge_shares,
        ))
    }

    /// Compute Greeks using entry volatility (EntryIV or EntryHV modes)
    fn compute_with_entry_vol(
        &self,
        spot: f64,
        volatility: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<(f64, PositionGreeks), String> {
        let computer = GreeksComputer::new(&self.trade, self.contract_multiplier, self.risk_free_rate);
        let greeks = computer.compute_with_flat_vol(spot, volatility, timestamp);
        Ok((volatility, greeks))
    }

    /// Compute Greeks using current market IV surface
    async fn compute_with_current_market_iv(
        &self,
        spot: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<(f64, PositionGreeks), String> {
        // Build IV surface
        let chain = self
            .options_repo
            .get_option_bars_at_time(&self.symbol, timestamp)
            .await
            .map_err(|e| e.to_string())?;

        let surface = build_iv_surface_minute_aligned(&chain, self.equity_repo.as_ref(), &self.symbol)
            .await
            .ok_or("Failed to build IV surface")?;

        // Get pricing model provider
        let pricing_model = PricingModel::StickyMoneyness; // Could be configurable
        let provider = pricing_model.to_provider();

        // Compute average IV for the position (for vega attribution)
        let computer = GreeksComputer::new(&self.trade, self.contract_multiplier, self.risk_free_rate);
        let iv = computer.compute_position_avg_iv(&surface, provider.as_ref(), timestamp);

        // Compute Greeks from surface
        let greeks = computer.compute_with_surface(spot, &surface, provider.as_ref(), timestamp);

        Ok((iv, greeks))
    }

    /// Compute Greeks using current HV
    async fn compute_with_current_hv(
        &self,
        spot: f64,
        timestamp: DateTime<Utc>,
        window: u32,
    ) -> Result<(f64, PositionGreeks), String> {
        // Get HV from price history
        let end_date = timestamp.date_naive();

        let bars = self
            .equity_repo
            .get_bars(&self.symbol, end_date)
            .await
            .map_err(|e| e.to_string())?;

        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();

        let hv = realized_volatility(&closes, window as usize, 252.0)
            .ok_or("Insufficient data for HV")?;

        // Compute Greeks with flat HV
        let computer = GreeksComputer::new(&self.trade, self.contract_multiplier, self.risk_free_rate);
        let greeks = computer.compute_with_flat_vol(spot, hv, timestamp);

        Ok((hv, greeks))
    }

    /// Convert snapshot times config to DateTime
    fn snapshot_times(&self, date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
        use cs_domain::datetime::eastern_to_utc;

        let (open_h, open_m, close_h, close_m) = match &self.config.snapshot_times {
            SnapshotTimes::OpenClose => (9, 30, 16, 0),
            SnapshotTimes::CloseOnly => (16, 0, 16, 0), // Same time
            SnapshotTimes::Custom {
                open_hour,
                open_minute,
                close_hour,
                close_minute,
            } => (*open_hour, *open_minute, *close_hour, *close_minute),
        };

        let open_time = eastern_to_utc(
            date,
            NaiveTime::from_hms_opt(open_h, open_m, 0).unwrap(),
        );
        let close_time = eastern_to_utc(
            date,
            NaiveTime::from_hms_opt(close_h, close_m, 0).unwrap(),
        );

        (open_time, close_time)
    }

    /// Build PositionAttribution from collected snapshots
    ///
    /// Returns None if no snapshots were collected
    pub fn build_attribution(&self, actual_pnl: Decimal) -> Option<PositionAttribution> {
        if self.snapshots.is_empty() {
            return None;
        }

        Some(PositionAttribution::from_snapshots(
            self.snapshots.clone(),
            actual_pnl,
        ))
    }

    /// Get number of snapshot pairs collected
    pub fn num_snapshots(&self) -> usize {
        self.snapshots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // TODO: Need mock repos
    fn test_hedge_timeline() {
        use cs_domain::{OptionLeg, PositionType, Strike};
        use finq_core::OptionType;
        use rust_decimal::Decimal;

        // Mock trade
        struct MockTrade {
            legs: Vec<(OptionLeg, PositionType)>,
        }

        impl CompositeTrade for MockTrade {
            fn legs(&self) -> &[(OptionLeg, PositionType)] {
                &self.legs
            }

            fn symbol(&self) -> &str {
                "SPY"
            }
        }

        let trade = MockTrade { legs: vec![] };

        let mut collector = SnapshotCollector::new(
            trade,
            Arc::new(crate::test_mocks::MockOptionsRepo::new()),
            Arc::new(crate::test_mocks::MockEquityRepo::new()),
            "SPY".to_string(),
            AttributionConfig::default(),
            100,
            0.05,
        );

        // Create hedge actions
        let hedges = vec![
            HedgeAction {
                timestamp: Utc::now(),
                shares: -50,
                spot_price: 100.0,
                delta_before: 50.0,
                delta_after: 0.0,
                cost: Decimal::ZERO,
            },
            HedgeAction {
                timestamp: Utc::now() + chrono::Duration::hours(1),
                shares: 20,
                spot_price: 102.0,
                delta_before: 30.0,
                delta_after: 10.0,
                cost: Decimal::ZERO,
            },
        ];

        collector.set_hedge_timeline(&hedges);

        // Check cumulative shares
        let t0 = Utc::now();
        let t1 = t0 + chrono::Duration::hours(1);
        let t2 = t1 + chrono::Duration::hours(1);

        assert_eq!(collector.hedge_shares_at(t0), -50);
        assert_eq!(collector.hedge_shares_at(t1), -30); // -50 + 20
        assert_eq!(collector.hedge_shares_at(t2), -30); // Still -30
    }

    #[test]
    #[ignore] // TODO: Need mock repos
    fn test_snapshot_times_open_close() {
        use cs_domain::{OptionLeg, PositionType};

        struct MockTrade {
            legs: Vec<(OptionLeg, PositionType)>,
        }

        impl CompositeTrade for MockTrade {
            fn legs(&self) -> &[(OptionLeg, PositionType)] {
                &self.legs
            }

            fn symbol(&self) -> &str {
                "SPY"
            }
        }

        let trade = MockTrade { legs: vec![] };

        let collector = SnapshotCollector::new(
            trade,
            Arc::new(crate::test_mocks::MockOptionsRepo::new()),
            Arc::new(crate::test_mocks::MockEquityRepo::new()),
            "SPY".to_string(),
            AttributionConfig {
                enabled: true,
                vol_source: VolatilitySource::CurrentMarketIV,
                snapshot_times: SnapshotTimes::OpenClose,
            },
            100,
            0.05,
        );

        let date = chrono::NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let (open, close) = collector.snapshot_times(date);

        // 9:30 ET = 14:30 UTC (EST)
        assert_eq!(open.time().hour(), 14);
        assert_eq!(open.time().minute(), 30);

        // 16:00 ET = 21:00 UTC (EST)
        assert_eq!(close.time().hour(), 21);
        assert_eq!(close.time().minute(), 0);
    }
}
