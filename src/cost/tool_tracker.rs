use super::types::ToolStats;
use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use parking_lot::{Mutex, MutexGuard};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsageRecord {
    pub id: String,
    pub tool: String,
    pub success: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub session_id: String,
}

impl ToolUsageRecord {
    fn new(session_id: impl Into<String>, tool: impl Into<String>, success: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            success,
            timestamp: chrono::Utc::now(),
            session_id: session_id.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolUsageSummary {
    pub session_total: usize,
    pub daily_total: usize,
    pub monthly_total: usize,
    pub by_tool: HashMap<String, ToolStats>,
}

pub struct ToolUsageTracker {
    storage: Arc<Mutex<ToolUsageStorage>>,
    session_id: String,
    session_total: Arc<Mutex<usize>>,
}

impl ToolUsageTracker {
    pub fn new(workspace_dir: &Path, tz: Tz) -> Result<Self> {
        let storage_path = resolve_storage_path(workspace_dir)?;
        let storage =
            ToolUsageStorage::new(&storage_path, tz).with_context(|| {
                format!("Failed to open tool usage storage at {}", storage_path.display())
            })?;

        Ok(Self {
            storage: Arc::new(Mutex::new(storage)),
            session_id: uuid::Uuid::new_v4().to_string(),
            session_total: Arc::new(Mutex::new(0)),
        })
    }

    fn lock_storage(&self) -> MutexGuard<'_, ToolUsageStorage> {
        self.storage.lock()
    }

    pub fn record(&self, tool: &str, success: bool) -> Result<()> {
        let record = ToolUsageRecord::new(&self.session_id, tool, success);

        {
            let mut storage = self.lock_storage();
            storage.add_record(record)?;
        }

        let mut session_total = self.session_total.lock();
        *session_total += 1;
        Ok(())
    }

    pub fn get_summary(&self) -> Result<ToolUsageSummary> {
        let mut storage = self.lock_storage();
        let (daily_total, monthly_total, by_tool) = storage.get_aggregates()?;
        let session_total = *self.session_total.lock();
        Ok(ToolUsageSummary {
            session_total,
            daily_total,
            monthly_total,
            by_tool,
        })
    }
}

fn resolve_storage_path(workspace_dir: &Path) -> Result<PathBuf> {
    let storage_path = workspace_dir.join("state").join("tool-usage.jsonl");
    if let Some(parent) = storage_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    Ok(storage_path)
}

struct ToolUsageStorage {
    path: PathBuf,
    daily_total: usize,
    monthly_total: usize,
    cached_day: NaiveDate,
    cached_year: i32,
    cached_month: u32,
    by_tool_monthly: HashMap<String, ToolStats>,
    last_len: u64,
    last_modified: Option<std::time::SystemTime>,
    tz: Tz,
}

impl ToolUsageStorage {
    fn new(path: &Path, tz: Tz) -> Result<Self> {
        let now = Utc::now().with_timezone(&tz);
        let cached_day = now.date_naive();
        let cached_year = now.year();
        let cached_month = now.month();
        let (last_len, last_modified) = file_state(path);
        let mut storage = Self {
            path: path.to_path_buf(),
            daily_total: 0,
            monthly_total: 0,
            cached_day,
            cached_year,
            cached_month,
            by_tool_monthly: HashMap::new(),
            last_len,
            last_modified,
            tz,
        };

        storage.reload_aggregates()?;
        Ok(storage)
    }

    fn reload_aggregates(&mut self) -> Result<()> {
        self.daily_total = 0;
        self.monthly_total = 0;
        self.by_tool_monthly.clear();

        if !self.path.exists() {
            return Ok(());
        }

        let file = OpenOptions::new().read(true).open(&self.path)?;
        let reader = BufReader::new(file);

        for (idx, line) in reader.lines().enumerate() {
            let line = line.unwrap_or_default();
            if line.trim().is_empty() {
                continue;
            }
            let record: ToolUsageRecord = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Skipping malformed tool usage record at {}:{}: {}",
                        self.path.display(),
                        idx + 1,
                        e
                    );
                    continue;
                }
            };

            let timestamp = record.timestamp.with_timezone(&self.tz);
            if timestamp.date_naive() == self.cached_day {
                self.daily_total += 1;
            }
            if timestamp.year() == self.cached_year && timestamp.month() == self.cached_month {
                self.monthly_total += 1;
                let entry = self.by_tool_monthly.entry(record.tool.clone()).or_insert_with(|| {
                    ToolStats {
                        tool: record.tool.clone(),
                        request_count: 0,
                        success_count: 0,
                        failure_count: 0,
                    }
                });
                entry.request_count += 1;
                if record.success {
                    entry.success_count += 1;
                } else {
                    entry.failure_count += 1;
                }
            }
        }

        Ok(())
    }

    fn refresh_cache_if_needed(&mut self) -> Result<()> {
        let now = Utc::now().with_timezone(&self.tz);
        let day = now.date_naive();
        let year = now.year();
        let month = now.month();
        let (len, modified) = file_state(&self.path);
        let file_changed = len != self.last_len || modified != self.last_modified;
        if day != self.cached_day || year != self.cached_year || month != self.cached_month || file_changed {
            self.cached_day = day;
            self.cached_year = year;
            self.cached_month = month;
            self.last_len = len;
            self.last_modified = modified;
            self.reload_aggregates()?;
        }
        Ok(())
    }

    fn add_record(&mut self, record: ToolUsageRecord) -> Result<()> {
        self.refresh_cache_if_needed()?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(&record)?)?;

        let timestamp = record.timestamp.with_timezone(&self.tz);
        if timestamp.date_naive() == self.cached_day {
            self.daily_total += 1;
        }
        if timestamp.year() == self.cached_year && timestamp.month() == self.cached_month {
            self.monthly_total += 1;
            let entry = self.by_tool_monthly.entry(record.tool.clone()).or_insert_with(|| {
                ToolStats {
                    tool: record.tool.clone(),
                    request_count: 0,
                    success_count: 0,
                    failure_count: 0,
                }
            });
            entry.request_count += 1;
            if record.success {
                entry.success_count += 1;
            } else {
                entry.failure_count += 1;
            }
        }

        Ok(())
    }

    fn get_aggregates(&mut self) -> Result<(usize, usize, HashMap<String, ToolStats>)> {
        self.refresh_cache_if_needed()?;
        Ok((
            self.daily_total,
            self.monthly_total,
            self.by_tool_monthly.clone(),
        ))
    }
}

fn file_state(path: &Path) -> (u64, Option<std::time::SystemTime>) {
    match fs::metadata(path) {
        Ok(meta) => (meta.len(), meta.modified().ok()),
        Err(_) => (0, None),
    }
}
