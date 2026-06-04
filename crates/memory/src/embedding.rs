use crate::types::MemoryNodeId;
use aegis_core::error::{AgentError, AgentResult};

/// Local embedding engine — pure Rust, zero C deps.
/// edgebert: MiniLM-L6-v2, 384d, ~23MB auto-download on first use.
pub struct Embedder {
    model: edgebert::Model,
}

impl Embedder {
    pub fn new() -> AgentResult<Self> {
        let model = edgebert::Model::from_pretrained(edgebert::ModelType::MiniLML6V2)
            .map_err(|e| AgentError::EmbeddingError(format!("edgebert init: {}", e)))?;
        Ok(Self { model })
    }

    pub fn embed_one(&self, text: &str) -> AgentResult<Vec<f32>> {
        let results = self.model.encode(vec![text], true)
            .map_err(|e| AgentError::EmbeddingError(format!("embed: {}", e)))?;
        results.into_iter().next()
            .ok_or_else(|| AgentError::EmbeddingError("no embedding returned".into()))
    }

    pub fn embed_batch(&self, texts: &[String]) -> AgentResult<Vec<Vec<f32>>> {
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        self.model.encode(refs, true)
            .map_err(|e| AgentError::EmbeddingError(format!("batch embed: {}", e)))
    }
}

/// Cosine similarity
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    edgebert::cosine_similarity(a, b)
}

pub fn knn_search(query: &[f32], candidates: &[(MemoryNodeId, Vec<f32>)], k: usize) -> Vec<(MemoryNodeId, f32)> {
    let mut scored: Vec<_> = candidates
        .iter()
        .map(|(id, emb)| (id.clone(), cosine_similarity(query, emb)))
        .collect();
    scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}
