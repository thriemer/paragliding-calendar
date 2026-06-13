use anyhow::Result;
use async_trait::async_trait;

use crate::domain::shared::{ActivitySuggestion, PlanningContext};

#[async_trait(?Send)]
pub trait ActivitySource: Send + Sync {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>>;
}
