/// The clock, in milliseconds since the epoch.
///
/// A clock before 1970 reads as zero rather than refusing: refusing to open a window over it would help nobody, and the
/// value only has to be monotonic within this process to be useful in a log line.
pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}
