use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::store::PersistentStore;

const DECISION_GRAPH_KEY: &str = "decision_graph";
const DEFAULT_GRAPH: &str = include_str!("flyable_decision_graph.json");

pub struct DecisionGraphRepository {
    store: Arc<PersistentStore>,
}

impl DecisionGraphRepository {
    pub fn new(store: Arc<PersistentStore>) -> Self {
        Self { store }
    }

    /// Returns the saved graph, or the compiled-in default if nothing has been saved yet.
    pub async fn load(&self) -> Result<Value> {
        if let Some(stored) = self.store.get::<String>(DECISION_GRAPH_KEY).await? {
            return Ok(serde_json::from_str(&stored)?);
        }
        Ok(serde_json::from_str(DEFAULT_GRAPH)?)
    }

    pub async fn save(&self, graph: &Value) -> Result<()> {
        let serialized = serde_json::to_string(graph)?;
        self.store.put(DECISION_GRAPH_KEY, serialized).await
    }
}
