//! Time and configuration parsing utilities for CLI arguments

use anyhow::{Context, Result};

/// Parse time in HH:MM format to (hour, minute) tuple
///
/// # Arguments
/// * `time_str` - Optional time string in HH:MM format (e.g., "09:35")
///
/// # Returns
/// * `Ok((Some(hour), Some(minute)))` if time string provided and valid
/// * `Ok((None, None))` if time string is None
/// * `Err(...)` if time string is invalid format or values out of range
///
/// # Validation
/// * Hour must be 0-23
/// * Minute must be 0-59
pub fn parse_time(time_str: Option<String>) -> Result<(Option<u32>, Option<u32>)> {
    match time_str {
        None => Ok((None, None)),
        Some(s) => {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid time format '{}'. Expected HH:MM (e.g., 09:35)", s);
            }

            let hour: u32 = parts[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid hour in time '{}'", s))?;
            let minute: u32 = parts[1]
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid minute in time '{}'", s))?;

            if hour > 23 {
                anyhow::bail!("Hour must be 0-23, got {}", hour);
            }
            if minute > 59 {
                anyhow::bail!("Minute must be 0-59, got {}", minute);
            }

            Ok((Some(hour), Some(minute)))
        }
    }
}

/// Parse delta range in "min,max" format
///
/// # Arguments
/// * `range_str` - Optional delta range string in format "min,max" (e.g., "0.25,0.75")
///
/// # Returns
/// * `Ok(Some((min, max)))` if range string provided and valid
/// * `Ok(None)` if range string is None
/// * `Err(...)` if range string is invalid format or values invalid
///
/// # Validation
/// * Range must contain exactly 2 comma-separated values
/// * Both values must be valid floats
pub fn parse_delta_range(range_str: Option<String>) -> Result<Option<(f64, f64)>> {
    match range_str {
        None => Ok(None),
        Some(s) => {
            let parts: Vec<&str> = s.split(',').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid delta range format. Use: --delta-range '0.25,0.75'");
            }

            let min: f64 = parts[0]
                .trim()
                .parse()
                .with_context(|| format!("Invalid delta range min: {}", parts[0]))?;
            let max: f64 = parts[1]
                .trim()
                .parse()
                .with_context(|| format!("Invalid delta range max: {}", parts[1]))?;

            Ok(Some((min, max)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_valid() {
        assert_eq!(parse_time(Some("09:35".to_string())).unwrap(), (Some(9), Some(35)));
        assert_eq!(parse_time(Some("23:59".to_string())).unwrap(), (Some(23), Some(59)));
        assert_eq!(parse_time(Some("00:00".to_string())).unwrap(), (Some(0), Some(0)));
    }

    #[test]
    fn test_parse_time_none() {
        assert_eq!(parse_time(None).unwrap(), (None, None));
    }

    #[test]
    fn test_parse_time_invalid_format() {
        assert!(parse_time(Some("9:35:00".to_string())).is_err());
        assert!(parse_time(Some("0935".to_string())).is_err());
        assert!(parse_time(Some("9".to_string())).is_err());
    }

    #[test]
    fn test_parse_time_invalid_hour() {
        assert!(parse_time(Some("24:00".to_string())).is_err());
        assert!(parse_time(Some("25:30".to_string())).is_err());
    }

    #[test]
    fn test_parse_time_invalid_minute() {
        assert!(parse_time(Some("12:60".to_string())).is_err());
        assert!(parse_time(Some("12:99".to_string())).is_err());
    }

    #[test]
    fn test_parse_delta_range_valid() {
        assert_eq!(parse_delta_range(Some("0.25,0.75".to_string())).unwrap(), Some((0.25, 0.75)));
        assert_eq!(parse_delta_range(Some("0.1,0.9".to_string())).unwrap(), Some((0.1, 0.9)));
        assert_eq!(parse_delta_range(Some("0.0,1.0".to_string())).unwrap(), Some((0.0, 1.0)));
    }

    #[test]
    fn test_parse_delta_range_with_spaces() {
        assert_eq!(parse_delta_range(Some("0.25 , 0.75".to_string())).unwrap(), Some((0.25, 0.75)));
    }

    #[test]
    fn test_parse_delta_range_none() {
        assert_eq!(parse_delta_range(None).unwrap(), None);
    }

    #[test]
    fn test_parse_delta_range_invalid_format() {
        assert!(parse_delta_range(Some("0.25".to_string())).is_err());
        assert!(parse_delta_range(Some("0.25,0.5,0.75".to_string())).is_err());
    }

    #[test]
    fn test_parse_delta_range_invalid_values() {
        assert!(parse_delta_range(Some("abc,0.75".to_string())).is_err());
        assert!(parse_delta_range(Some("0.25,xyz".to_string())).is_err());
    }
}
