// cs-domain/src/campaign/schedule.rs

use std::collections::BTreeMap;
use chrono::NaiveDate;
use crate::EarningsEvent;
use super::{TradingCampaign, TradingSession};

/// A schedule of trading sessions organized by date
///
/// This is the "date-centric" view for multi-stock execution.
/// Use this when you want to know "what trades happen on each date?"
#[derive(Debug, Clone)]
pub struct SessionSchedule {
    /// Sessions grouped by entry date
    by_entry_date: BTreeMap<NaiveDate, Vec<TradingSession>>,

    /// Sessions grouped by exit date (for tracking closes)
    by_exit_date: BTreeMap<NaiveDate, Vec<TradingSession>>,
}

impl SessionSchedule {
    /// Create an empty schedule
    pub fn new() -> Self {
        Self {
            by_entry_date: BTreeMap::new(),
            by_exit_date: BTreeMap::new(),
        }
    }

    /// Build schedule from multiple campaigns
    ///
    /// This is the primary constructor for multi-stock backtests.
    pub fn from_campaigns(
        campaigns: &[TradingCampaign],
        earnings_calendar: &[EarningsEvent],
    ) -> Self {
        let mut schedule = Self::new();

        for campaign in campaigns {
            let sessions = campaign.generate_sessions(earnings_calendar);
            for session in sessions {
                schedule.add_session(session);
            }
        }

        schedule
    }

    /// Add a session to the schedule
    pub fn add_session(&mut self, session: TradingSession) {
        self.by_entry_date
            .entry(session.entry_date())
            .or_default()
            .push(session.clone());

        self.by_exit_date
            .entry(session.exit_date())
            .or_default()
            .push(session);
    }

    /// Get sessions that ENTER on a given date
    pub fn entries_on(&self, date: NaiveDate) -> &[TradingSession] {
        self.by_entry_date.get(&date).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Get sessions that EXIT on a given date
    pub fn exits_on(&self, date: NaiveDate) -> &[TradingSession] {
        self.by_exit_date.get(&date).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Iterate over all entry dates in order
    pub fn iter_entry_dates(&self) -> impl Iterator<Item = (NaiveDate, &[TradingSession])> {
        self.by_entry_date.iter().map(|(d, s)| (*d, s.as_slice()))
    }

    /// Get all unique symbols in the schedule
    pub fn symbols(&self) -> Vec<String> {
        let mut symbols: Vec<String> = self.by_entry_date
            .values()
            .flat_map(|sessions| sessions.iter().map(|s| s.symbol.clone()))
            .collect();
        symbols.sort();
        symbols.dedup();
        symbols
    }

    /// Total number of sessions
    pub fn session_count(&self) -> usize {
        self.by_entry_date.values().map(Vec::len).sum()
    }

    /// Date range of the schedule
    pub fn date_range(&self) -> Option<(NaiveDate, NaiveDate)> {
        let first = self.by_entry_date.keys().next()?;
        let last = self.by_exit_date.keys().last()?;
        Some((*first, *last))
    }

    /// Filter to single symbol
    pub fn filter_symbol(&self, symbol: &str) -> Self {
        let mut filtered = Self::new();

        for sessions in self.by_entry_date.values() {
            for session in sessions.iter().filter(|s| s.symbol == symbol) {
                filtered.add_session(session.clone());
            }
        }

        filtered
    }

    /// Print summary
    pub fn summary(&self) -> String {
        let symbols = self.symbols();
        let (start, end) = self.date_range().unwrap_or((
            NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
        ));

        format!(
            "SessionSchedule: {} sessions for {} symbols ({} to {})",
            self.session_count(),
            symbols.len(),
            start,
            end,
        )
    }
}

impl Default for SessionSchedule {
    fn default() -> Self {
        Self::new()
    }
}
