use super::{parse_cost_timezone, CostTracker, ToolUsageTracker};
use crate::config::schema::CostConfig;
use anyhow::Result;
use std::path::Path;
use std::sync::{Arc, Mutex};

static COST_TRACKER: Mutex<Option<Arc<CostTracker>>> = Mutex::new(None);
static TOOL_TRACKER: Mutex<Option<Arc<ToolUsageTracker>>> = Mutex::new(None);

pub fn shared_cost_tracker(config: CostConfig, workspace_dir: &Path) -> Result<Arc<CostTracker>> {
    let mut guard = COST_TRACKER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard.as_ref() {
        return Ok(Arc::clone(existing));
    }
    let tracker = Arc::new(CostTracker::new(config, workspace_dir)?);
    *guard = Some(Arc::clone(&tracker));
    Ok(tracker)
}

pub fn shared_tool_usage_tracker(config: CostConfig, workspace_dir: &Path) -> Result<Arc<ToolUsageTracker>> {
    let mut guard = TOOL_TRACKER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard.as_ref() {
        return Ok(Arc::clone(existing));
    }
    let tz = parse_cost_timezone(&config.timezone);
    let tracker = Arc::new(ToolUsageTracker::new(workspace_dir, tz)?);
    *guard = Some(Arc::clone(&tracker));
    Ok(tracker)
}
