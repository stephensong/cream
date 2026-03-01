/// Chat session deposit for 30 minutes of text chat.
pub const CHAT_TEXT_DEPOSIT_CURD: u64 = 50;

/// Chat session deposit for 30 minutes of audio/video chat.
pub const CHAT_AV_DEPOSIT_CURD: u64 = 100;

/// Default session duration in minutes.
pub const CHAT_SESSION_MINUTES: u64 = 30;

/// Calculate the refund for unused session time.
/// Returns the CURD amount to refund based on elapsed seconds vs paid session duration.
pub fn calculate_refund(elapsed_secs: u64, deposit: u64, session_minutes: u64) -> u64 {
    let total_secs = session_minutes * 60;
    if elapsed_secs >= total_secs {
        return 0;
    }
    let remaining_secs = total_secs - elapsed_secs;
    // Proportional refund: deposit * remaining / total
    (deposit as u128 * remaining_secs as u128 / total_secs as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_session_no_refund() {
        assert_eq!(calculate_refund(1800, 50, 30), 0);
        assert_eq!(calculate_refund(2000, 50, 30), 0); // over time
    }

    #[test]
    fn zero_elapsed_full_refund() {
        assert_eq!(calculate_refund(0, 50, 30), 50);
    }

    #[test]
    fn half_session_half_refund() {
        assert_eq!(calculate_refund(900, 50, 30), 25);
    }

    #[test]
    fn proportional_refund() {
        // 10 min of 30 min → 20 min remaining → 2/3 refund
        assert_eq!(calculate_refund(600, 90, 30), 60);
    }
}
