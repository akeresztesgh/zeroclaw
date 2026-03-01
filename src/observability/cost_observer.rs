use crate::config::schema::CostConfig;
use crate::cost::{CostTracker, ToolUsageTracker, TokenUsage};
use crate::observability::traits::{Observer, ObserverEvent, ObserverMetric};
use std::any::Any;
use std::sync::Arc;

pub struct CostObserver {
    cost_tracker: Arc<CostTracker>,
    tool_tracker: Option<Arc<ToolUsageTracker>>,
    config: CostConfig,
}

impl CostObserver {
    pub fn new(
        cost_tracker: Arc<CostTracker>,
        tool_tracker: Option<Arc<ToolUsageTracker>>,
        config: CostConfig,
    ) -> Self {
        Self {
            cost_tracker,
            tool_tracker,
            config,
        }
    }
}

impl Observer for CostObserver {
    fn record_event(&self, event: &ObserverEvent) {
        match event {
            ObserverEvent::LlmResponse {
                model,
                success,
                input_tokens,
                output_tokens,
                ..
            } => {
                if !success {
                    return;
                }
                let (input_tokens, output_tokens) = match (input_tokens, output_tokens) {
                    (Some(i), Some(o)) => (*i, *o),
                    _ => return,
                };
                let pricing = self
                    .config
                    .prices
                    .get(model)
                    .unwrap_or(&self.config.fallback_pricing);
                let usage = TokenUsage::new(
                    model.clone(),
                    input_tokens,
                    output_tokens,
                    pricing.input,
                    pricing.output,
                );
                if let Err(err) = self.cost_tracker.record_usage(usage) {
                    tracing::warn!("Failed to record cost usage: {err}");
                }
            }
            ObserverEvent::ToolCall { tool, success, .. } => {
                if let Some(ref tracker) = self.tool_tracker {
                    if let Err(err) = tracker.record(tool, *success) {
                        tracing::warn!("Failed to record tool usage: {err}");
                    }
                }
            }
            _ => {}
        }
    }

    fn record_metric(&self, _metric: &ObserverMetric) {}

    fn name(&self) -> &str {
        "cost"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
