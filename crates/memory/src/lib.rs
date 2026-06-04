// aegis-memory: Causal memory system
// GAAMA graph + CraniMem gating + SYNAPSE retrieval + Dream consolidation

pub mod types;
pub mod schema;
pub mod store;
#[cfg(feature = "embedding")]
pub mod embedding;
#[cfg(feature = "embedding")]
pub mod retrieval;
pub mod episode;
pub mod gating;
pub mod consolidation;

pub use store::{MemoryStore, SqliteMemoryStore};
#[cfg(feature = "embedding")]
pub use embedding::Embedder;
#[cfg(feature = "embedding")]
pub use retrieval::retrieve;
pub use episode::{compute_error_signature, is_user_correction, EpisodeCloseResult, EpisodeManager};
pub use gating::CraniMemGater;
pub use consolidation::{ConsolidationConfig, ConsolidationResult, DreamConsolidator};
pub use types::*;
