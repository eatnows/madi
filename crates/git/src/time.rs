//! Human-friendly relative timestamps.

/// "5m ago", "3h ago", "2d ago", "4mo ago", "1y ago".
pub fn relative(now: i64, timestamp: i64) -> String {
    let minutes = ((now - timestamp) as f64 / 60.0).round() as i64;
    if minutes < 1 {
        return "just now".into();
    }
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = (minutes as f64 / 60.0).round() as i64;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = (hours as f64 / 24.0).round() as i64;
    if days < 30 {
        return format!("{days}d ago");
    }
    let months = (days as f64 / 30.0).round() as i64;
    if months < 12 {
        return format!("{months}mo ago");
    }
    format!("{}y ago", (months as f64 / 12.0).round() as i64)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_relative_times() {
        assert_eq!(relative(1000, 990), "just now");
        assert_eq!(relative(10_000, 10_000 - 5 * 60), "5m ago");
        assert_eq!(relative(100_000, 100_000 - 3 * 3600), "3h ago");
        assert_eq!(relative(10_000_000, 10_000_000 - 2 * 86_400), "2d ago");
    }
}
