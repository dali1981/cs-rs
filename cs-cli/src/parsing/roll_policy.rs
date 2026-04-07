//! Roll policy parsing utilities

use anyhow::{Context, Result};
use chrono::Weekday;
use cs_domain::RollPolicy;

/// Parse roll policy from string with optional roll day modifier
///
/// Supports three formats:
/// - "weekly": requires roll_day_modifier (monday-friday)
/// - "monthly": requires roll_day_modifier (week offset), only if allow_monthly=true
/// - "days:N": interval-based rolling
#[allow(dead_code)]
pub fn parse_roll_policy_impl(
    policy_str: &str,
    roll_day_modifier: Option<&str>,
    allow_monthly: bool,
) -> Result<RollPolicy> {
    match policy_str.to_lowercase().as_str() {
        "weekly" => {
            let day_str = roll_day_modifier.context("--roll-day is required for weekly roll policy")?;
            let weekday = match day_str.to_lowercase().as_str() {
                "monday" => Weekday::Mon,
                "tuesday" => Weekday::Tue,
                "wednesday" => Weekday::Wed,
                "thursday" => Weekday::Thu,
                "friday" => Weekday::Fri,
                _ => anyhow::bail!("Invalid --roll-day: {}. Must be monday-friday", day_str),
            };
            Ok(RollPolicy::Weekly { roll_day: weekday })
        }
        "monthly" if allow_monthly => {
            let offset_str = roll_day_modifier.context("--roll-day is required for monthly policy (use week offset, e.g., 0)")?;
            let offset = offset_str.parse::<i8>()
                .with_context(|| format!("Invalid month offset: {}. Expected integer week offset (e.g., 0 for 3rd Friday)", offset_str))?;
            Ok(RollPolicy::Monthly { roll_week_offset: offset })
        }
        s if s.starts_with("days:") => {
            let interval_str = &s[5..];
            let interval: u16 = interval_str.parse()
                .with_context(|| format!("Invalid interval in '{}'. Expected days:N (e.g., days:5)", policy_str))?;
            Ok(RollPolicy::TradingDays { interval })
        }
        "monthly" => anyhow::bail!("monthly policy not supported in this context. Use 'weekly' or 'days:N'"),
        _ if allow_monthly => anyhow::bail!("Unknown roll policy: {}. Use: weekly, monthly, or days:N", policy_str),
        _ => anyhow::bail!("Unknown roll policy: {}. Use: weekly or days:N", policy_str),
    }
}

/// Parse roll strategy from CLI arguments (backtest command - no monthly)
#[allow(dead_code)]
pub fn parse_roll_policy(strategy_str: &str, roll_day_str: Option<&str>) -> Result<RollPolicy> {
    parse_roll_policy_impl(strategy_str, roll_day_str, false)
}

/// Parse roll policy from campaign command arguments (allows monthly)
#[allow(dead_code)]
pub fn parse_campaign_roll_policy(roll_policy: &str, roll_day: &str) -> Result<RollPolicy> {
    parse_roll_policy_impl(roll_policy, Some(roll_day), true)
}
