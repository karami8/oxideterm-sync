use crate::rag::error::RagError;
use crate::rag::store::RagStore;
use crate::rag::types::EmbeddingRecord;

// ═══════════════════════════════════════════════════════════════════════════
// Vector Search
// ═══════════════════════════════════════════════════════════════════════════

/// A scored chunk from vector similarity search.
#[derive(Debug, Clone)]
pub struct VectorHit {
    pub chunk_id: String,
    pub score: f64,
}

/// Search by cosine similarity against stored embeddings.
/// `query_vector` comes from the provider's embedding API.
/// Only chunks within `collection_ids` are considered.
pub fn search_vector(
    store: &RagStore,
    query_vector: &[f32],
    collection_ids: &[String],
    top_k: usize,
) -> Result<Vec<VectorHit>, RagError> {
    if query_vector.is_empty() {
        return Ok(Vec::new());
    }

    // Get all chunk IDs in the target collections
    let chunk_ids = store.get_chunk_ids_in_collections(collection_ids)?;
    if chunk_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Fetch embeddings for these chunks
    let embeddings = store.get_embeddings_for_chunks(&chunk_ids)?;
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-compute query norm
    let query_norm = l2_norm(query_vector);
    if query_norm == 0.0 {
        return Ok(Vec::new());
    }

    // Score each embedding
    let mut hits: Vec<VectorHit> = embeddings
        .iter()
        .filter_map(|emb| {
            if emb.vector.len() != query_vector.len() {
                return None; // dimension mismatch
            }
            let score = cosine_similarity(query_vector, &emb.vector, query_norm);
            Some(VectorHit {
                chunk_id: emb.chunk_id.clone(),
                score,
            })
        })
        .collect();

    // Sort descending by score
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(top_k);

    Ok(hits)
}

/// Get chunk IDs that need embedding (not yet embedded).
pub fn get_pending_embeddings(
    store: &RagStore,
    collection_id: &str,
    limit: usize,
) -> Result<Vec<(String, String)>, RagError> {
    store.get_unembedded_chunk_ids(collection_id, limit)
}

/// Store embedding results from the provider.
pub fn store_embeddings(
    store: &RagStore,
    embeddings: Vec<EmbeddingRecord>,
) -> Result<usize, RagError> {
    let count = embeddings.len();
    store.store_embeddings_batch(&embeddings)?;
    Ok(count)
}

// ═══════════════════════════════════════════════════════════════════════════
// Math Utilities
// ═══════════════════════════════════════════════════════════════════════════

/// Cosine similarity with pre-computed query norm.
fn cosine_similarity(a: &[f32], b: &[f32], a_norm: f32) -> f64 {
    let b_norm = l2_norm(b);
    if b_norm == 0.0 {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    (dot / (a_norm * b_norm)) as f64
}

/// L2 (Euclidean) norm of a vector.
fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical() {
        let a = vec![1.0, 0.0, 1.0];
        let norm = l2_norm(&a);
        let sim = cosine_similarity(&a, &a, norm);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let norm = l2_norm(&a);
        let sim = cosine_similarity(&a, &b, norm);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let norm = l2_norm(&a);
        let sim = cosine_similarity(&a, &b, norm);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_l2_norm() {
        let v = vec![3.0, 4.0];
        assert!((l2_norm(&v) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_zero_vector() {
        let a = vec![1.0, 2.0];
        let b = vec![0.0, 0.0];
        let norm = l2_norm(&a);
        let sim = cosine_similarity(&a, &b, norm);
        assert_eq!(sim, 0.0);
    }
}
