/// Cost per chat message in CURD (same as storefront message toll).
pub const CHAT_MESSAGE_COST_CURD: u64 = 10;

/// Cost per A/V usage toll tick in CURD (charged while mic or camera is active).
pub const AV_TOLL_COST_CURD: u64 = 1;

/// Interval in seconds between A/V toll charges (10s for dev/testing, 60s for production).
pub const AV_TOLL_INTERVAL_SECS: u32 = 10;
