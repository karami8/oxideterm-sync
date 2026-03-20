//! RAG (Retrieval-Augmented Generation) Tauri Commands
//!
//! Provides commands for managing documentation collections,
//! indexing documents, storing embeddings, and searching.

use crate::rag::bm25;
use crate::rag::chunker;
use crate::rag::embedding;
use crate::rag::search::{self, SearchMode};
use crate::rag::store::RagStore;
use crate::rag::types::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tracing::info;

// ═══════════════════════════════════════════════════════════════════════════
// Request / Response Types
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollectionRequest {
    pub name: String,
    pub scope: DocScopeRequest,
}

#[derive(Debug, Deserialize)]
pub enum DocScopeRequest {
    Global,
    Connection { connection_id: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDocumentRequest {
    pub collection_id: String,
    pub title: String,
    pub content: String,
    pub format: String,
    pub source_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreEmbeddingsRequest {
    pub embeddings: Vec<EmbeddingInputRequest>,
    pub model_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingInputRequest {
    pub chunk_id: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub collection_ids: Vec<String>,
    pub query_vector: Option<Vec<f32>>,
    pub top_k: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    pub id: String,
    pub name: String,
    pub scope: DocScope,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentResponse {
    pub id: String,
    pub collection_id: String,
    pub title: String,
    pub source_path: Option<String>,
    pub format: String,
    pub chunk_count: usize,
    pub indexed_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    pub doc_count: usize,
    pub chunk_count: usize,
    pub embedded_chunk_count: usize,
    pub last_updated: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEmbeddingResponse {
    pub chunk_id: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultResponse {
    pub chunk_id: String,
    pub doc_id: String,
    pub doc_title: String,
    pub section_path: Option<String>,
    pub content: String,
    pub score: f64,
    pub source: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Tauri Commands
// ═══════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn rag_create_collection(
    store: State<'_, Arc<RagStore>>,
    request: CreateCollectionRequest,
) -> Result<CollectionResponse, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let scope = match request.scope {
        DocScopeRequest::Global => DocScope::Global,
        DocScopeRequest::Connection { connection_id } => DocScope::Connection(connection_id),
    };

    let collection = DocCollection {
        id: id.clone(),
        name: request.name,
        scope: scope.clone(),
        created_at: now,
        updated_at: now,
    };

    store.create_collection(&collection).map_err(|e| e.to_string())?;
    info!("Created RAG collection: {}", id);

    Ok(CollectionResponse {
        id: collection.id,
        name: collection.name,
        scope,
        created_at: now,
        updated_at: now,
    })
}

#[tauri::command]
pub async fn rag_list_collections(
    store: State<'_, Arc<RagStore>>,
    scope_filter: Option<String>,
) -> Result<Vec<CollectionResponse>, String> {
    let collections = store
        .list_collections(scope_filter.as_deref())
        .map_err(|e| e.to_string())?;

    Ok(collections
        .into_iter()
        .map(|c| CollectionResponse {
            id: c.id,
            name: c.name,
            scope: c.scope,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
        .collect())
}

#[tauri::command]
pub async fn rag_delete_collection(
    store: State<'_, Arc<RagStore>>,
    collection_id: String,
) -> Result<(), String> {
    store.delete_collection(&collection_id).map_err(|e| e.to_string())?;
    info!("Deleted RAG collection: {}", collection_id);
    Ok(())
}

#[tauri::command]
pub async fn rag_get_collection_stats(
    store: State<'_, Arc<RagStore>>,
    collection_id: String,
) -> Result<StatsResponse, String> {
    let stats = store
        .get_collection_stats(&collection_id)
        .map_err(|e| e.to_string())?;

    Ok(StatsResponse {
        doc_count: stats.doc_count,
        chunk_count: stats.chunk_count,
        embedded_chunk_count: stats.embedded_chunk_count,
        last_updated: stats.last_updated,
    })
}

#[tauri::command]
pub async fn rag_add_document(
    store: State<'_, Arc<RagStore>>,
    request: AddDocumentRequest,
) -> Result<DocumentResponse, String> {
    let doc_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    let format = match request.format.as_str() {
        "markdown" => DocFormat::Markdown,
        "plaintext" | "txt" => DocFormat::PlainText,
        other => return Err(format!("Unsupported format: {}", other)),
    };

    // Chunk the document
    let chunks = chunker::chunk_document(&doc_id, &request.content, &format);
    let content_hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        request.content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };

    let metadata = DocMetadata {
        id: doc_id.clone(),
        collection_id: request.collection_id.clone(),
        title: request.title.clone(),
        source_path: request.source_path,
        format: format.clone(),
        content_hash,
        indexed_at: now,
        chunk_count: chunks.len(),
    };

    // Store document + chunks
    store.add_document(&metadata, &chunks).map_err(|e| e.to_string())?;

    // Index chunks for BM25
    for chunk in &chunks {
        bm25::index_chunk(&store, &chunk.id, &chunk.content)
            .map_err(|e| e.to_string())?;
    }

    let format_str = match format {
        DocFormat::Markdown => "markdown",
        DocFormat::PlainText => "plaintext",
    };

    info!(
        "Added document '{}' ({} chunks) to collection {}",
        request.title,
        chunks.len(),
        request.collection_id
    );

    Ok(DocumentResponse {
        id: doc_id,
        collection_id: request.collection_id,
        title: request.title,
        source_path: metadata.source_path,
        format: format_str.to_string(),
        chunk_count: chunks.len(),
        indexed_at: now,
    })
}

#[tauri::command]
pub async fn rag_remove_document(
    store: State<'_, Arc<RagStore>>,
    doc_id: String,
) -> Result<(), String> {
    store.remove_document(&doc_id).map_err(|e| e.to_string())?;
    info!("Removed RAG document: {}", doc_id);
    Ok(())
}

#[tauri::command]
pub async fn rag_list_documents(
    store: State<'_, Arc<RagStore>>,
    collection_id: String,
) -> Result<Vec<DocumentResponse>, String> {
    let doc_ids = store
        .get_collection_doc_ids(&collection_id)
        .map_err(|e| e.to_string())?;

    let mut docs = Vec::new();
    for doc_id in doc_ids {
        if let Some(meta) = store.get_doc_metadata(&doc_id).map_err(|e| e.to_string())? {
            let format_str = match meta.format {
                DocFormat::Markdown => "markdown",
                DocFormat::PlainText => "plaintext",
            };
            docs.push(DocumentResponse {
                id: meta.id,
                collection_id: meta.collection_id,
                title: meta.title,
                source_path: meta.source_path,
                format: format_str.to_string(),
                chunk_count: meta.chunk_count,
                indexed_at: meta.indexed_at,
            });
        }
    }
    Ok(docs)
}

#[tauri::command]
pub async fn rag_get_pending_embeddings(
    store: State<'_, Arc<RagStore>>,
    collection_id: String,
    limit: Option<usize>,
) -> Result<Vec<PendingEmbeddingResponse>, String> {
    let pending = embedding::get_pending_embeddings(&store, &collection_id, limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;

    Ok(pending
        .into_iter()
        .map(|(chunk_id, content)| PendingEmbeddingResponse { chunk_id, content })
        .collect())
}

#[tauri::command]
pub async fn rag_store_embeddings(
    store: State<'_, Arc<RagStore>>,
    request: StoreEmbeddingsRequest,
) -> Result<usize, String> {
    let records: Vec<EmbeddingRecord> = request
        .embeddings
        .into_iter()
        .map(|e| {
            let dimensions = e.vector.len();
            EmbeddingRecord {
                chunk_id: e.chunk_id,
                vector: e.vector,
                model_name: request.model_name.clone(),
                dimensions,
            }
        })
        .collect();

    let count = embedding::store_embeddings(&store, records).map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub async fn rag_search(
    store: State<'_, Arc<RagStore>>,
    request: SearchRequest,
) -> Result<Vec<SearchResultResponse>, String> {
    let top_k = request.top_k.unwrap_or(5);

    let mode = match request.query_vector {
        Some(vec) => SearchMode::Hybrid { query_vector: vec },
        None => SearchMode::KeywordOnly,
    };

    let results = search::search(&store, &request.query, &request.collection_ids, mode, top_k)
        .map_err(|e| e.to_string())?;

    Ok(results
        .into_iter()
        .map(|r| {
            let source_str = match r.source {
                SearchSource::Bm25Only => "bm25",
                SearchSource::VectorOnly => "vector",
                SearchSource::Both => "both",
            };
            SearchResultResponse {
                chunk_id: r.chunk_id,
                doc_id: r.doc_id,
                doc_title: r.doc_title,
                section_path: r.section_path,
                content: r.content,
                score: r.score,
                source: source_str.to_string(),
            }
        })
        .collect())
}

#[tauri::command]
pub async fn rag_reindex_collection(
    store: State<'_, Arc<RagStore>>,
    collection_id: String,
) -> Result<usize, String> {
    let count = bm25::reindex_collection(&store, &collection_id)
        .map_err(|e| e.to_string())?;
    info!("Re-indexed collection {}: {} chunks", collection_id, count);
    Ok(count)
}
