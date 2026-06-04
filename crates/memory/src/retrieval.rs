use crate::embedding::{knn_search, Embedder};
use crate::store::MemoryStore;
use crate::types::*;
use aegis_core::error::AgentResult;
use chrono::Utc;
use std::collections::HashMap;

/// 3-tier hybrid retrieval: vector k-NN + graph PPR + time-decay merge
pub fn retrieve(
    query: &str,
    embedder: &Embedder,
    store: &impl MemoryStore,
    limit: usize,
) -> AgentResult<Vec<RetrievedMemory>> {
    // Tier 1: vector k-NN
    let query_emb = embedder.embed_one(query)?;
    let all_embeddings = store.get_all_embeddings(None)?;
    let semantic_hits = knn_search(&query_emb, &all_embeddings, limit * 2);

    // Tier 2: graph PPR from top-3 anchors
    let mut graph_scores: HashMap<MemoryNodeId, f32> = HashMap::new();
    for (anchor_id, _) in semantic_hits.iter().take(3) {
        let neighborhood = store.get_neighborhood(anchor_id, 2)?;
        for (node_id, edge_type, confidence) in neighborhood {
            let edge_weight = match edge_type {
                MemoryEdgeType::CausedBy => 1.5,
                MemoryEdgeType::FixedBy => 1.4,
                MemoryEdgeType::LearnedFrom => 1.2,
                MemoryEdgeType::Contradicted => 1.3,
                MemoryEdgeType::Supersedes => 1.1,
                MemoryEdgeType::SupportedBy => 1.0,
                MemoryEdgeType::SimilarTo => 0.8,
                MemoryEdgeType::PrerequisiteOf => 0.6,
            };
            *graph_scores.entry(node_id).or_insert(0.0) += edge_weight * confidence;
        }
    }

    // Tier 3: merge + time-decay + rank
    let now = Utc::now();
    let mut merged: Vec<RetrievedMemory> = Vec::new();

    for (node_id, sem_score) in &semantic_hits {
        let graph_score = graph_scores.get(node_id).copied().unwrap_or(0.0);
        let node = match store.get_node(node_id)? {
            Some(n) => n,
            None => continue,
        };

        let age_days = (now - node.created_at()).num_days() as f32;
        let decay_factor = 0.5f32.powf(age_days / 30.0); // 30-day halflife

        let utility: f32 = match &node {
            MemoryNode::Insight(i) => i.utility_score,
            _ => 0.5,
        };
        let combined = sem_score * 0.5 + graph_score * 0.3 + utility * 0.2;
        let final_score = combined * decay_factor;

        if final_score > 0.3 {
            let source_ep_ids = store.find_supporting_episodes(node.id()).unwrap_or_default();
            merged.push(RetrievedMemory {
                node_id: node_id.clone(),
                node_type: node.node_type(),
                content: node.content_summary(),
                score: final_score,
                semantic_score: *sem_score,
                graph_score,
                source_episode_ids: source_ep_ids,
                confidence: match &node {
                    MemoryNode::Insight(i) => i.confidence,
                    MemoryNode::RootCause(r) => r.confidence,
                    _ => 0.5,
                },
                last_updated: node.created_at(),
            });
        }
    }

    merged.sort_unstable_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(limit);
    Ok(merged)
}
