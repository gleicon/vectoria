pub mod aggregation;
pub mod embedding;
pub mod engine;
pub mod model;
pub mod search;
pub mod storage;
pub mod vector;

pub use engine::{SearchEngineBuilder, SearchEngineSync};
pub use search::SearchEngine;

pub(crate) fn dir_bytes(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}
