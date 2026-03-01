pub mod tracker;
pub mod tool_tracker;
pub mod types;
mod shared;
use std::str::FromStr;

// Re-exported for potential external use (public API)
#[allow(unused_imports)]
pub use tracker::CostTracker;
pub use tool_tracker::ToolUsageTracker;
#[allow(unused_imports)]
pub use types::{BudgetCheck, CostRecord, CostSummary, ModelStats, TokenUsage, UsagePeriod};
pub use shared::{shared_cost_tracker, shared_tool_usage_tracker};

pub fn parse_cost_timezone(value: &str) -> chrono_tz::Tz {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return chrono_tz::UTC;
    }
    let normalized = trimmed
        .replace("usa/", "America/")
        .replace("US/", "America/")
        .replace("us/", "America/")
        .replace(' ', "_");
    if let Ok(tz) = chrono_tz::Tz::from_str(&normalized) {
        return tz;
    }
    match trimmed.to_lowercase().as_str() {
        "us/eastern" | "est" | "edt" | "eastern" => chrono_tz::America::New_York,
        _ => chrono_tz::UTC,
    }
}
