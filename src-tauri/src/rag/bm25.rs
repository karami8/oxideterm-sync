use crate::rag::chunker::estimate_tokens;
use crate::rag::error::RagError;
use crate::rag::store::RagStore;
use crate::rag::types::{is_cjk, Bm25Stats, PostingEntry};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

// ═══════════════════════════════════════════════════════════════════════════
// BM25 Parameters
// ═══════════════════════════════════════════════════════════════════════════

const K1: f64 = 1.2;
const B: f64 = 0.75;

// ═══════════════════════════════════════════════════════════════════════════
// Tokenizer — character bigram for CJK, whitespace split for ASCII
// ═══════════════════════════════════════════════════════════════════════════

/// Tokenize text into a set of terms for BM25 indexing.
/// CJK characters are split into overlapping character bigrams.
/// ASCII/Latin text is split on whitespace and lowercased.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii_buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let ch = chars[i];

        if is_cjk(ch) {
            // Flush any accumulated ASCII token
            flush_ascii(&mut ascii_buf, &mut tokens);

            // Emit character bigrams
            if i + 1 < len && is_cjk(chars[i + 1]) {
                let mut bigram = String::with_capacity(8);
                bigram.push(ch);
                bigram.push(chars[i + 1]);
                tokens.push(bigram);
            } else {
                // Single CJK char (end of run) — emit as unigram
                tokens.push(ch.to_string());
            }
            i += 1;
        } else if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            ascii_buf.push(ch.to_ascii_lowercase());
            i += 1;
        } else {
            // Whitespace or punctuation — flush ASCII buffer
            flush_ascii(&mut ascii_buf, &mut tokens);
            i += 1;
        }
    }
    flush_ascii(&mut ascii_buf, &mut tokens);
    tokens
}

/// Compute term frequency map: term → count.
pub fn term_frequencies(tokens: &[String]) -> HashMap<String, f32> {
    let mut freqs: HashMap<String, f32> = HashMap::new();
    for t in tokens {
        *freqs.entry(t.clone()).or_insert(0.0) += 1.0;
    }
    freqs
}

fn flush_ascii(buf: &mut String, tokens: &mut Vec<String>) {
    if !buf.is_empty() {
        // Filter out very short tokens (single char) that are noise
        if buf.len() >= 2 {
            tokens.push(buf.clone());
        }
        buf.clear();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BM25 Index Building
// ═══════════════════════════════════════════════════════════════════════════

/// Index a single chunk's content into BM25 postings.
/// Call this after adding a document to the store.
pub fn index_chunk(
    store: &RagStore,
    chunk_id: &str,
    content: &str,
) -> Result<(), RagError> {
    let tokens = tokenize(content);
    let doc_length = estimate_tokens(content);
    let tf_map = term_frequencies(&tokens);

    store.add_to_bm25_index(&tf_map, chunk_id, doc_length)?;
    Ok(())
}

/// Re-index BM25 from scratch. The `_collection_id` parameter is retained
/// for API compatibility but the index is always rebuilt globally.
pub fn reindex_collection(
    store: &RagStore,
    _collection_id: &str,
) -> Result<usize, RagError> {
    reindex_all(store, None, None)
}

/// Rebuild the global BM25 index from all collections.
///
/// `cancel` — if provided, checked periodically; if set to `true`, the
///   rebuild aborts early and returns `RagError::Cancelled`.
/// `on_progress` — called with (current, total) after each chunk is processed.
pub fn reindex_all(
    store: &RagStore,
    cancel: Option<&AtomicBool>,
    mut on_progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<usize, RagError> {
    let all_col_ids = store.get_all_collection_ids()?;
    let chunk_ids = store.get_chunk_ids_in_collections(&all_col_ids)?;
    let total = chunk_ids.len();

    let mut postings: HashMap<String, Vec<PostingEntry>> = HashMap::new();
    let mut total_dl: f64 = 0.0;
    let mut count: usize = 0;

    for (idx, cid) in chunk_ids.iter().enumerate() {
        // Check cancellation
        if let Some(flag) = cancel {
            if flag.load(Ordering::Relaxed) {
                return Err(RagError::Cancelled);
            }
        }

        if let Some(chunk) = store.get_chunk(cid)? {
            let tokens = tokenize(&chunk.content);
            let doc_length = estimate_tokens(&chunk.content);
            let tf_map = term_frequencies(&tokens);

            for (term, tf) in &tf_map {
                postings
                    .entry(term.clone())
                    .or_default()
                    .push(PostingEntry {
                        chunk_id: cid.clone(),
                        tf: *tf,
                        doc_length,
                    });
            }

            total_dl += doc_length as f64;
            count += 1;
        }

        // Report progress
        if let Some(ref mut cb) = on_progress {
            cb(idx + 1, total);
        }
    }

    let avg_dl = if count > 0 {
        total_dl / count as f64
    } else {
        0.0
    };
    let stats = Bm25Stats {
        doc_count: count,
        avg_dl,
    };

    store.write_bm25_index(&postings, &stats)?;
    Ok(count)
}

// ═══════════════════════════════════════════════════════════════════════════
// BM25 Search
// ═══════════════════════════════════════════════════════════════════════════

/// A scored chunk from BM25 search.
#[derive(Debug, Clone)]
pub struct Bm25Hit {
    pub chunk_id: String,
    pub score: f64,
}

/// Search using BM25 scoring, returning top-K results.
pub fn search_bm25(
    store: &RagStore,
    query: &str,
    collection_ids: &[String],
    top_k: usize,
) -> Result<Vec<Bm25Hit>, RagError> {
    let query_tokens = tokenize(query);
    let query_terms: Vec<String> = query_tokens
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let stats = store.get_bm25_stats()?;
    let (doc_count, avg_dl) = match stats {
        Some(s) => (s.doc_count, s.avg_dl),
        None => return Ok(Vec::new()),
    };

    if doc_count == 0 || avg_dl <= 0.0 {
        return Ok(Vec::new());
    }

    // Subset of chunk IDs in the target collections (for filtering)
    let valid_chunks: std::collections::HashSet<String> = if collection_ids.is_empty() {
        std::collections::HashSet::new()
    } else {
        store
            .get_chunk_ids_in_collections(collection_ids)?
            .into_iter()
            .collect()
    };

    let filter = !collection_ids.is_empty();

    // Accumulate scores
    let mut scores: HashMap<String, f64> = HashMap::new();

    for term in &query_terms {
        let postings = store.get_bm25_postings(term)?;
        if postings.is_empty() {
            continue;
        }

        // IDF: log((N - df + 0.5) / (df + 0.5) + 1)
        let df = postings.len() as f64;
        let idf = ((doc_count as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();

        for entry in &postings {
            if filter && !valid_chunks.contains(&entry.chunk_id) {
                continue;
            }

            // BM25 TF component: (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * dl/avgdl))
            let tf = entry.tf as f64;
            let dl = if entry.doc_length > 0 {
                entry.doc_length as f64
            } else {
                // Fallback for legacy postings without stored doc_length
                avg_dl
            };
            let tf_norm = (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * dl / avg_dl));

            *scores.entry(entry.chunk_id.clone()).or_insert(0.0) += idf * tf_norm;
        }
    }

    // Sort by score descending
    let mut hits: Vec<Bm25Hit> = scores
        .into_iter()
        .map(|(chunk_id, score)| Bm25Hit { chunk_id, score })
        .collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(top_k);

    Ok(hits)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_english() {
        let tokens = tokenize("Hello world, this is a test!");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // Single chars like "a" should be filtered
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_tokenize_cjk_bigrams() {
        let tokens = tokenize("运维文档");
        // Should produce bigrams: "运维", "维文", "文档"
        assert!(tokens.contains(&"运维".to_string()));
        assert!(tokens.contains(&"维文".to_string()));
        assert!(tokens.contains(&"文档".to_string()));
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize("Docker 部署指南 version 2.0");
        assert!(tokens.contains(&"docker".to_string()));
        assert!(tokens.contains(&"部署".to_string()));
        assert!(tokens.contains(&"署指".to_string()));
        assert!(tokens.contains(&"指南".to_string()));
        assert!(tokens.contains(&"version".to_string()));
        // "2.0" → "2" (single char filtered) + "0" (filtered)
    }

    #[test]
    fn test_term_frequencies() {
        let tokens = vec![
            "hello".to_string(),
            "world".to_string(),
            "hello".to_string(),
        ];
        let tf = term_frequencies(&tokens);
        assert_eq!(tf.get("hello"), Some(&2.0));
        assert_eq!(tf.get("world"), Some(&1.0));
    }

    #[test]
    fn test_empty_query() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }
}
