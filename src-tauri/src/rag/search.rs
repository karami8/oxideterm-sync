use crate::rag::bm25::{search_bm25, Bm25Hit};
use crate::rag::embedding::{search_vector, VectorHit};
use crate::rag::error::RagError;
use crate::rag::store::RagStore;
use crate::rag::types::{SearchResult, SearchSource};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

/// Reciprocal Rank Fusion constant.
const RRF_K: f64 = 60.0;
/// Number of candidates from each retrieval path before fusion.
const CANDIDATES_PER_PATH: usize = 20;

// ═══════════════════════════════════════════════════════════════════════════
// Hybrid Search
// ═══════════════════════════════════════════════════════════════════════════

/// Search mode determining which retrieval paths to use.
pub enum SearchMode {
    /// BM25 keyword search only (no embeddings required).
    KeywordOnly,
    /// Full hybrid: BM25 + vector similarity with RRF fusion.
    Hybrid { query_vector: Vec<f32> },
}

/// Perform a hybrid search across the given collections.
///
/// Steps:
///   1. BM25 keyword search → Top-20 candidates
///   2. (If hybrid) Vector similarity → Top-20 candidates
///   3. Reciprocal Rank Fusion (k=60) → merged ranking
///   4. Enrich with chunk metadata → Top-K results
pub fn search(
    store: &RagStore,
    query: &str,
    collection_ids: &[String],
    mode: SearchMode,
    top_k: usize,
) -> Result<Vec<SearchResult>, RagError> {
    // Phase 1: BM25
    let bm25_hits = search_bm25(store, query, collection_ids, CANDIDATES_PER_PATH)?;

    // Phase 2: Vector (optional)
    let vector_hits = match &mode {
        SearchMode::KeywordOnly => Vec::new(),
        SearchMode::Hybrid { query_vector } => {
            search_vector(store, query_vector, collection_ids, CANDIDATES_PER_PATH)?
        }
    };

    // Phase 3: Fuse results
    let fused = rrf_fuse(&bm25_hits, &vector_hits);

    // Phase 4: Enrich and return top-K
    let mut results = Vec::with_capacity(top_k.min(fused.len()));
    for (chunk_id, score, source) in fused.into_iter().take(top_k) {
        if let Some(chunk) = store.get_chunk(&chunk_id)? {
            let doc_title = store
                .get_doc_metadata(&chunk.doc_id)?
                .map(|m| m.title)
                .unwrap_or_default();

            results.push(SearchResult {
                chunk_id: chunk_id.clone(),
                doc_id: chunk.doc_id,
                doc_title,
                section_path: chunk.section_path,
                content: chunk.content,
                score,
                source,
            });
        }
    }

    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════
// Reciprocal Rank Fusion
// ═══════════════════════════════════════════════════════════════════════════

/// Reciprocal Rank Fusion: merge two ranked lists.
/// Returns (chunk_id, fused_score, source) sorted by fused score descending.
fn rrf_fuse(
    bm25_hits: &[Bm25Hit],
    vector_hits: &[VectorHit],
) -> Vec<(String, f64, SearchSource)> {
    let mut scores: HashMap<String, (f64, bool, bool)> = HashMap::new();

    // BM25 contributions
    for (rank, hit) in bm25_hits.iter().enumerate() {
        let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
        let entry = scores.entry(hit.chunk_id.clone()).or_insert((0.0, false, false));
        entry.0 += rrf_score;
        entry.1 = true; // seen in BM25
    }

    // Vector contributions
    for (rank, hit) in vector_hits.iter().enumerate() {
        let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
        let entry = scores.entry(hit.chunk_id.clone()).or_insert((0.0, false, false));
        entry.0 += rrf_score;
        entry.2 = true; // seen in vector
    }

    let mut results: Vec<(String, f64, SearchSource)> = scores
        .into_iter()
        .map(|(id, (score, in_bm25, in_vector))| {
            let source = match (in_bm25, in_vector) {
                (true, true) => SearchSource::Both,
                (true, false) => SearchSource::Bm25Only,
                (false, true) => SearchSource::VectorOnly,
                (false, false) => unreachable!(),
            };
            (id, score, source)
        })
        .collect();

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_fuse_overlap() {
        let bm25 = vec![
            Bm25Hit { chunk_id: "a".into(), score: 5.0 },
            Bm25Hit { chunk_id: "b".into(), score: 3.0 },
            Bm25Hit { chunk_id: "c".into(), score: 1.0 },
        ];
        let vector = vec![
            VectorHit { chunk_id: "b".into(), score: 0.95 },
            VectorHit { chunk_id: "d".into(), score: 0.80 },
            VectorHit { chunk_id: "a".into(), score: 0.60 },
        ];

        let fused = rrf_fuse(&bm25, &vector);

        // "a" and "b" appear in both lists, should have higher combined scores
        let a_score = fused.iter().find(|(id, _, _)| id == "a").unwrap().1;
        let b_score = fused.iter().find(|(id, _, _)| id == "b").unwrap().1;
        let c_score = fused.iter().find(|(id, _, _)| id == "c").unwrap().1;
        let d_score = fused.iter().find(|(id, _, _)| id == "d").unwrap().1;

        // Items in both lists should score higher than items in one
        assert!(a_score > c_score);
        assert!(b_score > d_score);

        // Check source annotations
        let a_source = &fused.iter().find(|(id, _, _)| id == "a").unwrap().2;
        assert!(matches!(a_source, SearchSource::Both));
        let c_source = &fused.iter().find(|(id, _, _)| id == "c").unwrap().2;
        assert!(matches!(c_source, SearchSource::Bm25Only));
        let d_source = &fused.iter().find(|(id, _, _)| id == "d").unwrap().2;
        assert!(matches!(d_source, SearchSource::VectorOnly));
    }

    #[test]
    fn test_rrf_fuse_empty() {
        let fused = rrf_fuse(&[], &[]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_rrf_bm25_only() {
        let bm25 = vec![
            Bm25Hit { chunk_id: "x".into(), score: 2.0 },
        ];
        let fused = rrf_fuse(&bm25, &[]);
        assert_eq!(fused.len(), 1);
        assert!(matches!(fused[0].2, SearchSource::Bm25Only));
    }
}
