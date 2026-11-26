use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use uuid::Uuid;

const TRACE_CAPACITY: usize = 1_000;

/// One stage of the search pipeline with its timing and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStage {
    pub name: String,
    pub duration_ms: u64,
    /// Stage-specific data: result counts, model used, etc.
    pub data: serde_json::Value,
}

/// Full trace for a single query. Persisted when `explain: true` in the request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTrace {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub query: String,
    pub effective_query: String,
    pub index: String,
    pub stages: Vec<TraceStage>,
    pub total_ms: u64,
}

/// In-memory trace store. Bounded ring buffer of the last `TRACE_CAPACITY` traces.
/// Thread-safe; shared across request handlers via `Arc<TraceStore>`.
pub struct TraceStore {
    ring: Mutex<VecDeque<QueryTrace>>,
}

impl TraceStore {
    pub fn new() -> Self {
        Self { ring: Mutex::new(VecDeque::with_capacity(TRACE_CAPACITY)) }
    }

    pub fn push(&self, trace: QueryTrace) {
        let mut ring = self.ring.lock().unwrap();
        if ring.len() >= TRACE_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(trace);
    }

    pub fn get(&self, id: &Uuid) -> Option<QueryTrace> {
        let ring = self.ring.lock().unwrap();
        ring.iter().find(|t| &t.id == id).cloned()
    }
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::new()
    }
}
