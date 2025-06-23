// src/inverted_index.rs

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering}; // Required for LruCache capacity

use colored::*;
use regex;

use serde::{Deserialize, Serialize};

use bincode;
use bincode::serde as bincode_serde;

use rayon::prelude::*; // FIXED: Added this import for parallel processing

use lru::LruCache;
use std::sync::{Arc, Mutex};

// --- TYPE ALIASES for complex types to simplify declarations and aid compiler parsing ---
type TermPostings = Vec<(u32, usize)>;
type DocumentPartialIndex = HashMap<String, TermPostings>;
type ProcessedDocumentResult = Result<(Document, DocumentPartialIndex), io::Error>;

// --- STRUCTS ---
#[derive(Debug, Clone, Serialize, Deserialize)] // Use serde's Serialize/Deserialize directly
pub struct Document {
    pub id: u32,
    pub path: PathBuf,
    pub content: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub doc: Document,
    pub score: f64,
    pub snippet: String,
}

// Helper function for default LruCache initialization
fn default_search_cache() -> Arc<Mutex<LruCache<String, Vec<SearchResult>>>> {
    // This function provides a default instance for Serde when deserializing,
    // even though the field is skipped. The actual cache capacity will be
    // re-initialized correctly in `from_serialized_data`.
    // We use a dummy non-zero value here, as it will be immediately overwritten.
    let non_zero_capacity = NonZeroUsize::new(1).expect("Capacity must be non-zero");
    Arc::new(Mutex::new(LruCache::new(non_zero_capacity)))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvertedIndex {
    index: HashMap<String, TermPostings>, // Using type alias
    documents: HashMap<u32, Document>,
    #[serde(skip)] // Atomic types cannot be directly serialized by serde
    next_doc_id: AtomicU32,
    total_docs: usize,
    #[serde(skip, default = "default_search_cache")] // FIXED: Add default function for Serde
    search_cache: Arc<Mutex<LruCache<String, Vec<SearchResult>>>>,
    // REMOVED: #[serde(skip, default)] - cache_capacity now gets serialized normally
    cache_capacity: usize,
}

impl InvertedIndex {
    pub fn new() -> Self {
        const DEFAULT_CACHE_CAPACITY: usize = 100; // Define capacity here
        // Ensure capacity is non-zero, unwrap is safe as 100 is not zero
        let non_zero_capacity = NonZeroUsize::new(DEFAULT_CACHE_CAPACITY).unwrap();
        InvertedIndex {
            index: HashMap::new(),
            documents: HashMap::new(),
            next_doc_id: AtomicU32::new(1),
            total_docs: 0,
            search_cache: Arc::new(Mutex::new(LruCache::new(non_zero_capacity))),
            cache_capacity: DEFAULT_CACHE_CAPACITY,
        }
    }

    // Persistence Methods
    pub fn from_serialized_data(serialized_data: &[u8]) -> Result<Self, Box<dyn Error>> {
        let (mut index, _bytes_read): (InvertedIndex, usize) =
            bincode_serde::decode_from_slice(serialized_data, bincode::config::standard())?;

        let max_id = index.documents.keys().max().copied().unwrap_or(0);
        // Re-initialize the AtomicU32 as it was skipped during serialization
        index.next_doc_id = AtomicU32::new(max_id + 1);
        // Re-initialize the cache as it was skipped during serialization
        // Ensure capacity is non-zero, with proper error handling for safety
        let non_zero_capacity = NonZeroUsize::new(index.cache_capacity).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Cache capacity cannot be zero")
        })?;
        index.search_cache = Arc::new(Mutex::new(LruCache::new(non_zero_capacity)));

        Ok(index)
    }

    pub fn to_serialized_data(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let encoded_data = bincode_serde::encode_to_vec(self, bincode::config::standard())?;
        Ok(encoded_data)
    }

    #[allow(dead_code)] // Added to suppress the unused method warning
    pub fn add_document(&mut self, doc: Document) {
        let doc_id = self.next_doc_id.fetch_add(1, Ordering::SeqCst);
        let current_doc = Document {
            id: doc_id,
            path: doc.path,
            content: doc.content,
            title: doc.title,
        };

        let tokens = crate::tokenizer::tokenize(&current_doc.content);
        let mut doc_term_frequencies: HashMap<String, usize> = HashMap::new();
        for token in tokens {
            *doc_term_frequencies.entry(token).or_insert(0) += 1;
        }

        for (token, count) in doc_term_frequencies {
            self.index
                .entry(token)
                .or_insert_with(Vec::new)
                .push((doc_id, count));
        }

        self.documents.insert(doc_id, current_doc);
        self.total_docs += 1;
        self.clear_cache(); // Clear cache when index is modified
    }

    fn clear_cache(&self) {
        let mut cache = self.search_cache.lock().unwrap();
        cache.clear();
    }

    // Refactored search function to explicitly manage 'results' assignment
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query_tokens = crate::tokenizer::tokenize(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        // --- Caching Logic ---
        {
            let mut cache = self.search_cache.lock().unwrap(); // Acquire lock
            if let Some(results) = cache.get(query) {
                return results.clone(); // Return cloned results from cache
            }
        } // Lock is automatically released here when `cache` goes out of scope

        // If not in cache, perform the actual search and assign to 'results'
        let results = self.perform_search_and_rank(&query_tokens, query);

        // Store in cache after computation
        {
            let mut cache = self.search_cache.lock().unwrap(); // Acquire lock
            cache.put(query.to_string(), results.clone());
        } // Lock automatically released here

        results // Return the computed and cached results
    }

    // Helper function to separate the core search and ranking logic
    fn perform_search_and_rank(
        &self,
        query_tokens: &[String],
        original_query: &str,
    ) -> Vec<SearchResult> {
        let mut candidate_docs: HashMap<u32, HashMap<String, usize>> = HashMap::new();

        for token in query_tokens {
            if let Some(doc_entries) = self.index.get(token) {
                for (doc_id, tf) in doc_entries {
                    candidate_docs
                        .entry(*doc_id)
                        .or_insert_with(HashMap::new)
                        .insert(token.clone(), *tf);
                }
            } else {
                // If any query token is not found, no results
                return Vec::new(); // Return empty early
            }
        }

        let mut intersection_results: HashMap<u32, HashMap<String, usize>> = HashMap::new();
        for (doc_id, term_map) in candidate_docs {
            let mut all_terms_present = true;
            for q_token in query_tokens {
                if !term_map.contains_key(q_token) {
                    all_terms_present = false;
                    break;
                }
            }
            if all_terms_present {
                intersection_results.insert(doc_id, term_map);
            }
        }

        let mut ranked_results: Vec<(f64, u32)> = Vec::new();

        for (doc_id, term_frequencies) in intersection_results {
            let mut score = 0.0;
            for q_token in query_tokens {
                let tf = *term_frequencies.get(q_token).unwrap_or(&0) as f64;

                if tf == 0.0 {
                    continue;
                }

                let num_docs_with_term = self.index.get(q_token).map_or(0, |v| v.len()) as f64;

                let idf = if num_docs_with_term > 0.0 {
                    (self.total_docs as f64 / num_docs_with_term).log10()
                } else {
                    0.0
                };

                score += tf * idf;
            }
            ranked_results.push((score, doc_id));
        }

        ranked_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let original_query_terms: Vec<String> = original_query
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        ranked_results
            .into_iter()
            .filter_map(|(score, doc_id)| {
                self.documents.get(&doc_id).cloned().map(|doc| {
                    let content_lower = doc.content.to_lowercase();

                    let mut first_match_idx = None;
                    for q_token_stemmed in query_tokens {
                        if let Some(idx) = content_lower.find(q_token_stemmed) {
                            first_match_idx = Some(idx);
                            break;
                        }
                    }

                    let snippet;

                    if let Some(start_char_idx) = first_match_idx {
                        let context_start = start_char_idx.saturating_sub(50);
                        let context_end =
                            (start_char_idx + query_tokens[0].len() + 50).min(content_lower.len());

                        let mut byte_start = 0;
                        for (i, (byte_idx, _)) in doc.content.char_indices().enumerate() {
                            if i == context_start {
                                byte_start = byte_idx;
                                break;
                            }
                        }
                        let mut byte_end = doc.content.len();
                        for (i, (byte_idx, _)) in doc.content.char_indices().enumerate() {
                            if i == context_end {
                                byte_end = byte_idx;
                                break;
                            }
                        }

                        let snippet_text = &doc.content[byte_start..byte_end];
                        let mut highlighted_snippet = snippet_text.to_string();

                        for original_term in &original_query_terms {
                            let re_str = format!(r"(?i)\b{}\b", regex::escape(original_term));
                            let re = regex::Regex::new(&re_str).unwrap();

                            highlighted_snippet = re
                                .replace_all(&highlighted_snippet, |caps: &regex::Captures| {
                                    caps[0].red().bold().to_string()
                                })
                                .to_string();
                        }
                        snippet = format!("...{}...", highlighted_snippet);
                    } else {
                        snippet = format!("{}...", &doc.content[..doc.content.len().min(150)]);
                    }

                    SearchResult {
                        doc,
                        score,
                        snippet,
                    }
                })
            })
            .collect()
    }

    pub fn load_documents_from_directory(&mut self, path: &Path) -> io::Result<()> {
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Provided path is not a directory",
            ));
        }

        // Collect all file paths first
        let file_paths: Vec<PathBuf> = fs::read_dir(path)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_path = entry.path();
                if file_path.is_file() && file_path.extension().map_or(false, |ext| ext == "txt") {
                    Some(file_path)
                } else {
                    None
                }
            })
            .collect();

        // Use a temporary AtomicU32 for parallel ID generation
        let temp_next_doc_id = AtomicU32::new(self.next_doc_id.load(Ordering::SeqCst));

        // Process documents in parallel using the new type alias
        let results: Vec<ProcessedDocumentResult> = file_paths
            .par_iter() // This makes the iterator parallel
            .map(|file_path| {
                println!("Indexing document ID (temp): {:?}", file_path); // temp ID for debug
                let doc_id = temp_next_doc_id.fetch_add(1, Ordering::SeqCst);
                let content = fs::read_to_string(&file_path)?;
                let title = file_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let doc = Document {
                    id: doc_id, // Assign the new ID
                    path: file_path.clone(),
                    content: content,
                    title: title,
                };

                let tokens = crate::tokenizer::tokenize(&doc.content);
                let mut doc_term_frequencies: HashMap<String, usize> = HashMap::new();
                for token in tokens {
                    *doc_term_frequencies.entry(token).or_insert(0) += 1;
                }

                let mut partial_index_entries = HashMap::new();
                for (token, count) in doc_term_frequencies {
                    partial_index_entries
                        .entry(token)
                        .or_insert_with(Vec::new)
                        .push((doc_id, count));
                }

                Ok((doc, partial_index_entries))
            })
            .collect(); // Collect results from parallel processing

        // Merge results back into the main InvertedIndex on the main thread
        let mut new_docs_count = 0;
        for result in results {
            let (doc, partial_index_entries) = result?; // Propagate errors
            let doc_id = doc.id; // Get the assigned ID

            self.documents.insert(doc_id, doc);
            new_docs_count += 1;

            for (token, postings) in partial_index_entries {
                self.index
                    .entry(token)
                    .or_insert_with(Vec::new)
                    .extend(postings); // Extend with the postings for this doc
            }
        }

        self.total_docs += new_docs_count;
        self.next_doc_id = temp_next_doc_id; // Update the main AtomicU32
        self.clear_cache(); // Clear cache after indexing
        Ok(())
    }

    pub fn total_documents(&self) -> usize {
        self.total_docs
    }
}
