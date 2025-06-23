// src/inverted_index.rs

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use colored::*;
use regex;
use strsim;

use serde::{Deserialize, Serialize};

use bincode;
use bincode::serde as bincode_serde;

use rayon::prelude::*;

use lru::LruCache;
use std::sync::{Arc, Mutex};

// --- CONSTANTS ---
const FUZZY_THRESHOLD: usize = 2; // Maximum Levenshtein distance for fuzzy matching

// --- TYPE ALIASES ---
type TermPostings = Vec<(u32, Vec<usize>)>;
type DocumentPartialIndex = HashMap<String, Vec<usize>>;
type ProcessedDocumentResult = Result<(Document, DocumentPartialIndex), io::Error>;

// --- STRUCTS ---
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let non_zero_capacity = NonZeroUsize::new(1).expect("Capacity must be non-zero");
    Arc::new(Mutex::new(LruCache::new(non_zero_capacity)))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvertedIndex {
    index: HashMap<String, TermPostings>,
    documents: HashMap<u32, Document>,
    #[serde(skip)]
    next_doc_id: AtomicU32,
    total_docs: usize,
    #[serde(skip, default = "default_search_cache")]
    search_cache: Arc<Mutex<LruCache<String, Vec<SearchResult>>>>,
    cache_capacity: usize,
}

impl InvertedIndex {
    pub fn new() -> Self {
        const DEFAULT_CACHE_CAPACITY: usize = 100;
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
        index.next_doc_id = AtomicU32::new(max_id + 1);
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

    #[allow(dead_code)]
    pub fn add_document(&mut self, doc: Document) {
        let doc_id = self.next_doc_id.fetch_add(1, Ordering::SeqCst);
        let current_doc = Document {
            id: doc_id,
            path: doc.path,
            content: doc.content,
            title: doc.title,
        };

        let tokens_with_positions = crate::tokenizer::tokenize(&current_doc.content);
        let mut doc_token_positions: HashMap<String, Vec<usize>> = HashMap::new();
        for (token, pos) in tokens_with_positions {
            doc_token_positions
                .entry(token)
                .or_insert_with(Vec::new)
                .push(pos);
        }

        for (token, positions) in doc_token_positions {
            self.index
                .entry(token)
                .or_insert_with(Vec::new)
                .push((doc_id, positions));
        }

        self.documents.insert(doc_id, current_doc);
        self.total_docs += 1;
        self.clear_cache();
    }

    fn clear_cache(&self) {
        let mut cache = self.search_cache.lock().unwrap();
        cache.clear();
    }

    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        {
            let mut cache = self.search_cache.lock().unwrap();
            if let Some(results) = cache.get(query) {
                return results.clone();
            }
        }

        let results = if query.starts_with('"') && query.ends_with('"') && query.len() > 1 {
            let phrase_content = &query[1..query.len() - 1];
            self.perform_phrase_search_and_rank(phrase_content, query)
        } else {
            let query_tokens_with_pos = crate::tokenizer::tokenize(query);
            let query_tokens: Vec<String> = query_tokens_with_pos
                .iter()
                .map(|(s, _)| s.clone())
                .collect();
            self.perform_keyword_search_and_rank(&query_tokens, query)
        };

        {
            let mut cache = self.search_cache.lock().unwrap();
            cache.put(query.to_string(), results.clone());
        }

        results
    }

    // NEW: Helper function to find fuzzy matches for a token
    fn find_fuzzy_matches(&self, query_token: &str) -> Vec<(String, usize)> {
        let mut fuzzy_matches = Vec::new();
        for (indexed_term, _) in &self.index {
            let distance = strsim::levenshtein(query_token, indexed_term);
            if distance <= FUZZY_THRESHOLD {
                fuzzy_matches.push((indexed_term.clone(), distance));
            }
        }
        // Sort by distance so closest matches are considered first
        fuzzy_matches.sort_by_key(|(_, distance)| *distance);
        fuzzy_matches
    }

    fn perform_keyword_search_and_rank(
        &self,
        query_tokens: &[String],
        original_query: &str,
    ) -> Vec<SearchResult> {
        let mut candidate_docs: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();
        let mut fuzzy_matched_terms: HashMap<String, String> = HashMap::new();

        for token in query_tokens {
            if let Some(doc_entries) = self.index.get(token) {
                // Exact match
                for (doc_id, positions) in doc_entries {
                    candidate_docs
                        .entry(*doc_id)
                        .or_insert_with(HashMap::new)
                        .insert(token.clone(), positions.clone());
                }
            } else {
                // No exact match, try fuzzy matching
                let matches = self.find_fuzzy_matches(token);
                if let Some((closest_match, distance)) = matches.into_iter().next() {
                    // Take the closest one
                    if let Some(doc_entries) = self.index.get(&closest_match) {
                        for (doc_id, positions) in doc_entries {
                            candidate_docs
                                .entry(*doc_id)
                                .or_insert_with(HashMap::new)
                                .insert(closest_match.clone(), positions.clone());
                        }
                        fuzzy_matched_terms.insert(token.clone(), closest_match.clone());
                        println!(
                            "Note: Fuzzy matched '{}' to '{}' (distance: {})",
                            token.yellow(),
                            closest_match.yellow(),
                            distance
                        );
                    } else {
                        return Vec::new();
                    }
                } else {
                    println!(
                        "No matches (exact or fuzzy) found for term: {}",
                        token.red()
                    );
                    return Vec::new();
                }
            }
        }

        let mut intersection_results: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();
        for (doc_id, term_map) in candidate_docs {
            let mut all_terms_present = true;
            for q_token in query_tokens {
                if !term_map.contains_key(q_token)
                    && !term_map
                        .contains_key(fuzzy_matched_terms.get(q_token).unwrap_or(&String::new()))
                {
                    all_terms_present = false;
                    break;
                }
            }
            if all_terms_present {
                intersection_results.insert(doc_id, term_map);
            }
        }

        let mut ranked_results: Vec<(f64, u32)> = Vec::new();

        for (doc_id, term_frequencies_and_pos) in intersection_results {
            let mut score = 0.0;
            for q_token in query_tokens {
                let actual_term = fuzzy_matched_terms.get(q_token).unwrap_or(q_token);

                let tf = term_frequencies_and_pos
                    .get(actual_term)
                    .map_or(0, |v| v.len()) as f64;

                if tf == 0.0 {
                    continue;
                }

                let num_docs_with_term = self.index.get(actual_term).map_or(0, |v| v.len()) as f64;

                let idf = if num_docs_with_term > 0.0 {
                    (self.total_docs as f64 / num_docs_with_term).log10()
                } else {
                    0.0
                };

                let mut term_score = tf * idf;

                // Penalize fuzzy matches
                if fuzzy_matched_terms.contains_key(q_token) {
                    term_score *= 0.5;
                }

                score += term_score;
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
                    for q_token in query_tokens {
                        let actual_term_for_snippet =
                            fuzzy_matched_terms.get(q_token).unwrap_or(q_token);
                        if let Some(idx) = content_lower.find(actual_term_for_snippet) {
                            first_match_idx = Some(idx);
                            break;
                        }
                    }

                    let snippet = if let Some(start_char_idx) = first_match_idx {
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
                        format!("...{}...", highlighted_snippet)
                    } else {
                        format!("{}...", &doc.content[..doc.content.len().min(150)])
                    };

                    SearchResult {
                        doc,
                        score,
                        snippet,
                    }
                })
            })
            .collect()
    }

    fn perform_phrase_search_and_rank(
        &self,
        phrase_query_text: &str,
        original_query: &str,
    ) -> Vec<SearchResult> {
        let query_tokens_with_pos = crate::tokenizer::tokenize(phrase_query_text);

        if query_tokens_with_pos.is_empty() {
            return Vec::new();
        }

        let query_stemmed_tokens: Vec<String> = query_tokens_with_pos
            .iter()
            .map(|(s, _)| s.clone())
            .collect();

        // Find documents that contain all query tokens
        let mut common_docs_data: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();

        for (token_idx, token) in query_stemmed_tokens.iter().enumerate() {
            if let Some(doc_entries) = self.index.get(token) {
                if token_idx == 0 {
                    // For the first token, initialize common_docs_data with its documents
                    for (doc_id, positions) in doc_entries {
                        common_docs_data
                            .entry(*doc_id)
                            .or_insert_with(HashMap::new)
                            .insert(token.clone(), positions.clone());
                    }
                } else {
                    let current_matches_for_token: HashMap<u32, Vec<usize>> = doc_entries
                        .iter()
                        .map(|(id, pos)| (*id, pos.clone()))
                        .collect();

                    common_docs_data
                        .retain(|doc_id, _| current_matches_for_token.contains_key(doc_id));

                    for (doc_id, positions) in current_matches_for_token {
                        if let Some(doc_token_map) = common_docs_data.get_mut(&doc_id) {
                            doc_token_map.insert(token.clone(), positions);
                        }
                    }
                }
            } else {
                return Vec::new();
            }
        }

        let mut phrase_matching_docs: HashMap<u32, f64> = HashMap::new();

        for (doc_id, doc_tokens_pos_map) in common_docs_data {
            if let Some(first_token_positions) = doc_tokens_pos_map.get(&query_stemmed_tokens[0]) {
                for &start_pos in first_token_positions {
                    let mut is_phrase_match = true;
                    for i in 1..query_stemmed_tokens.len() {
                        let current_query_token = &query_stemmed_tokens[i];
                        let expected_pos = start_pos + (i as usize);

                        if let Some(doc_token_positions) =
                            doc_tokens_pos_map.get(current_query_token)
                        {
                            if !doc_token_positions.contains(&expected_pos) {
                                is_phrase_match = false;
                                break;
                            }
                        } else {
                            is_phrase_match = false;
                            break;
                        }
                    }

                    if is_phrase_match {
                        *phrase_matching_docs.entry(doc_id).or_insert(0.0) += 1.0;
                    }
                }
            }
        }

        let mut ranked_results: Vec<(f64, u32)> = phrase_matching_docs
            .into_iter()
            .map(|(doc_id, score)| (score, doc_id))
            .collect();
        ranked_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let original_phrase_terms: Vec<String> = original_query
            .trim_matches('"')
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        ranked_results
            .into_iter()
            .filter_map(|(score, doc_id)| {
                self.documents.get(&doc_id).cloned().map(|doc| {
                    let content_lower = doc.content.to_lowercase();
                    let snippet_highlight_target = phrase_query_text.to_lowercase();

                    let snippet = if let Some(first_match_idx) =
                        content_lower.find(&snippet_highlight_target)
                    {
                        let context_start = first_match_idx.saturating_sub(50);
                        let context_end = (first_match_idx + snippet_highlight_target.len() + 50)
                            .min(content_lower.len());

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

                        for original_term in &original_phrase_terms {
                            let re_str = format!(r"(?i)\b{}\b", regex::escape(original_term));
                            let re = regex::Regex::new(&re_str).unwrap();

                            highlighted_snippet = re
                                .replace_all(&highlighted_snippet, |caps: &regex::Captures| {
                                    caps[0].red().bold().to_string()
                                })
                                .to_string();
                        }
                        format!("...{}...", highlighted_snippet)
                    } else {
                        format!("{}...", &doc.content[..doc.content.len().min(150)])
                    };

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

        let temp_next_doc_id = AtomicU32::new(self.next_doc_id.load(Ordering::SeqCst));

        let results: Vec<ProcessedDocumentResult> = file_paths
            .par_iter()
            .map(|file_path| {
                println!("Indexing document ID (temp): {:?}", file_path);
                let doc_id = temp_next_doc_id.fetch_add(1, Ordering::SeqCst);
                let content = fs::read_to_string(&file_path)?;
                let title = file_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let doc = Document {
                    id: doc_id,
                    path: file_path.clone(),
                    content: content,
                    title: title,
                };

                let tokens_with_positions = crate::tokenizer::tokenize(&doc.content);
                let mut partial_index_entries: HashMap<String, Vec<usize>> = HashMap::new();
                for (token, pos) in tokens_with_positions {
                    partial_index_entries
                        .entry(token)
                        .or_insert_with(Vec::new)
                        .push(pos);
                }
                Ok((doc, partial_index_entries))
            })
            .collect();

        let mut new_docs_count = 0;
        for result in results {
            let (doc, partial_index_entries) = result?;
            let doc_id = doc.id;

            self.documents.insert(doc_id, doc);
            new_docs_count += 1;

            for (token, positions) in partial_index_entries {
                self.index
                    .entry(token)
                    .or_insert_with(Vec::new)
                    .push((doc_id, positions));
            }
        }

        self.total_docs += new_docs_count;
        self.next_doc_id = temp_next_doc_id;
        self.clear_cache();
        Ok(())
    }

    pub fn total_documents(&self) -> usize {
        self.total_docs
    }
}
