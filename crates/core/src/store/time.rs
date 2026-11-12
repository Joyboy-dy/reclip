//! Store timestamps: UTC RFC 3339 with milliseconds and a `Z` suffix.

/// Current UTC time as RFC 3339 with millisecond precision and a `Z` suffix.
///
/// Timestamps are compared as strings in SQL (`expires_at <= ?`, ordering by
/// `created_at`), which is only correct when every value is UTC and uses this
/// exact format. Use this function for every timestamp written to the store.
pub fn now() -> String {
    timestamp(chrono::Utc::now())
}

/// [`now`] plus `d`, in the same format (saturating far in the future).
pub fn now_plus(d: std::time::Duration) -> String {
    timestamp(plus(chrono::Utc::now(), d))
}

/// `at + d`, saturating far in the future.
fn plus(
    at: chrono::DateTime<chrono::Utc>,
    d: std::time::Duration,
) -> chrono::DateTime<chrono::Utc> {
    chrono::TimeDelta::from_std(d)
        .ok()
        .and_then(|d| at.checked_add_signed(d))
        .unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC)
}

/// `finished_at` and `expires_at = finished_at + ttl`, from one instant.
pub(super) fn finished_and_expiry(ttl: std::time::Duration) -> (String, String) {
    let at = chrono::Utc::now();
    (timestamp(at), timestamp(plus(at, ttl)))
}

/// [`now`] minus `d`, in the same format.
pub fn now_minus(d: std::time::Duration) -> String {
    let at = chrono::TimeDelta::from_std(d)
        .ok()
        .and_then(|d| chrono::Utc::now().checked_sub_signed(d))
        .unwrap_or(chrono::DateTime::<chrono::Utc>::MIN_UTC);
    timestamp(at)
}

fn timestamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
