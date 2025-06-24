// src/inverted_index.rs

use std::collections::HashMap;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::UNIX_EPOCH;

use colored::*;
use regex;
use strsim;

use serde::{Deserialize, Serialize};
use serde_json;

use bincode;
use bincode::serde as bincode_serde;

use lru::LruCache;
use std::sync::{Arc, Mutex};

use scraper::{Html, Selector};

use pdf_extract::extract_text;

use anyhow::{Context, Result, anyhow};

// --- CONSTANTS ---
const FUZZY_THRESHOLD: usize = 2;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

// --- TYPE ALIASES ---
type TermPostings = Vec<(u32, Vec<usize>)>;
type DocumentPartialIndex = HashMap<String, Vec<usize>>;
type ProcessedDocumentResult = Result<(Document, DocumentPartialIndex)>;

// --- STRUCTS ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: u32,
    pub path: PathBuf,
    pub content: String,
    pub title: String,
    pub tags: Vec<String>,
    pub num_tokens: usize,
    pub modified_time: u64,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub doc: Document,
    pub score: f64,
    pub snippet: String,
    pub tags: Vec<String>,
}

// Structs for graph data serialization
#[derive(Serialize, Debug)]
pub struct GraphNode {
    pub id: u32,
    pub label: String,
    pub title: String,
    pub group: String,
    pub content_preview: String,
    pub js_tags: Vec<String>, // Direct tags for JavaScript filtering
}

#[derive(Serialize, Debug)]
pub struct GraphEdge {
    pub from: u32,
    pub to: u32,
    pub width: f64,
}

#[derive(Serialize, Debug)]
pub struct ClientSearchableDocument {
    pub id: u32,
    pub title: String,
    pub content: String, // Full content for client-side search
    pub tags: Vec<String>,
    pub content_preview: String, // Keep preview for quick display
}

// Master data structure for the full web application
#[derive(Serialize, Debug)]
pub struct FullWebAppData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub searchable_documents: HashMap<u32, ClientSearchableDocument>,
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
    tags: HashMap<String, Vec<u32>>,
    #[serde(skip)]
    next_doc_id: AtomicU32,
    pub total_docs: usize,
    pub avg_doc_length: f64,
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
            tags: HashMap::new(),
            next_doc_id: AtomicU32::new(1),
            total_docs: 0,
            avg_doc_length: 0.0,
            search_cache: Arc::new(Mutex::new(LruCache::new(non_zero_capacity))),
            cache_capacity: DEFAULT_CACHE_CAPACITY,
        }
    }

    // Persistence Methods
    pub fn from_serialized_data(serialized_data: &[u8]) -> Result<Self> {
        let (mut index, _bytes_read): (InvertedIndex, usize) =
            bincode_serde::decode_from_slice(serialized_data, bincode::config::standard())
                .context("Failed to decode index data from slice")?;

        let max_id = index.documents.keys().max().copied().unwrap_or(0);
        index.next_doc_id = AtomicU32::new(max_id + 1);
        let non_zero_capacity =
            NonZeroUsize::new(index.cache_capacity).context("Cache capacity cannot be zero")?;
        index.search_cache = Arc::new(Mutex::new(LruCache::new(non_zero_capacity)));

        Ok(index)
    }

    pub fn to_serialized_data(&self) -> Result<Vec<u8>> {
        let encoded_data = bincode_serde::encode_to_vec(self, bincode::config::standard())
            .context("Failed to encode index data to vector")?;
        Ok(encoded_data)
    }

    #[allow(dead_code)]
    pub fn add_document(&mut self, doc: Document) {
        let doc_id = doc.id;

        let current_doc = Document {
            id: doc_id,
            path: doc.path,
            content: doc.content,
            title: doc.title,
            tags: doc.tags.clone(),
            num_tokens: doc.num_tokens,
            modified_time: doc.modified_time,
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

        for tag in &current_doc.tags {
            self.tags
                .entry(tag.clone())
                .or_insert_with(Vec::new)
                .push(doc_id);
        }

        self.documents.insert(doc_id, current_doc);
        self.clear_cache();
    }

    fn remove_document(&mut self, doc_id: u32) {
        if let Some(doc_to_remove) = self.documents.remove(&doc_id) {
            let tokens = crate::tokenizer::tokenize(&doc_to_remove.content);
            for (token, _) in tokens {
                if let Some(postings) = self.index.get_mut(&token) {
                    postings.retain(|&(id, _)| id != doc_id);
                    if postings.is_empty() {
                        self.index.remove(&token);
                    }
                }
            }

            for tag in &doc_to_remove.tags {
                if let Some(doc_ids) = self.tags.get_mut(tag) {
                    doc_ids.retain(|&id| id != doc_id);
                    if doc_ids.is_empty() {
                        self.tags.remove(tag);
                    }
                }
            }
            self.clear_cache();
        }
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

        let results = if query.starts_with('#') {
            let tag_name = query[1..].trim().to_lowercase();
            if tag_name.is_empty() {
                return Vec::new();
            }

            let mut tag_results: Vec<SearchResult> = Vec::new();
            if let Some(doc_ids) = self.tags.get(&tag_name) {
                for &doc_id in doc_ids {
                    if let Some(doc) = self.documents.get(&doc_id) {
                        let snippet = "...".to_string();
                        tag_results.push(SearchResult {
                            doc: doc.clone(),
                            score: 1.0,
                            snippet: snippet,
                            tags: doc.tags.clone(),
                        });
                    }
                }
            }
            tag_results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            tag_results
        } else if query.starts_with('"') && query.ends_with('"') && query.len() > 1 {
            let phrase_content = &query[1..query.len() - 1];
            self.perform_phrase_search_and_rank(phrase_content, query)
        } else {
            let mut processed_query_terms: Vec<(String, bool)> = Vec::new();

            for raw_word in query.to_lowercase().split_whitespace() {
                let clean_word =
                    raw_word.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '*');

                if clean_word.ends_with('*') && clean_word.len() > 1 {
                    let prefix = &clean_word[0..clean_word.len() - 1];
                    let stemmed_prefix_tokens = crate::tokenizer::tokenize(prefix);

                    let mut found_wildcard_matches = false;
                    for (stemmed_prefix_part, _) in stemmed_prefix_tokens {
                        for indexed_term in self.index.keys() {
                            if indexed_term.starts_with(&stemmed_prefix_part) {
                                processed_query_terms.push((indexed_term.clone(), true));
                                found_wildcard_matches = true;
                            }
                        }
                    }
                    if !found_wildcard_matches {
                        if query.split_whitespace().count() == 1 && processed_query_terms.is_empty()
                        {
                            return Vec::new();
                        }
                    }
                } else {
                    let normal_tokens = crate::tokenizer::tokenize(clean_word);
                    for (token, _) in normal_tokens {
                        if !token.is_empty() {
                            processed_query_terms.push((token, false));
                        }
                    }
                }
            }

            if processed_query_terms.is_empty() {
                return Vec::new();
            }

            self.perform_keyword_search_and_rank(&processed_query_terms, query)
        };

        {
            let mut cache = self.search_cache.lock().unwrap();
            cache.put(query.to_string(), results.clone());
        }

        results
    }

    fn find_fuzzy_matches(&self, query_token: &str) -> Vec<(String, usize)> {
        let mut fuzzy_matches = Vec::new();
        for (indexed_term, _) in &self.index {
            let distance = strsim::levenshtein(query_token, indexed_term);
            if distance <= FUZZY_THRESHOLD {
                fuzzy_matches.push((indexed_term.clone(), distance));
            }
        }
        fuzzy_matches.sort_by_key(|(_, distance)| *distance);
        fuzzy_matches
    }

    fn perform_keyword_search_and_rank(
        &self,
        processed_query_terms: &[(String, bool)],
        _original_query: &str,
    ) -> Vec<SearchResult> {
        let mut candidate_docs: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();
        let mut fuzzy_matched_terms: HashMap<String, String> = HashMap::new();

        for (token, is_wildcard_origin) in processed_query_terms {
            if let Some(doc_entries) = self.index.get(token) {
                for (doc_id, positions) in doc_entries {
                    candidate_docs
                        .entry(*doc_id)
                        .or_insert_with(HashMap::new)
                        .insert(token.clone(), positions.clone());
                }
            } else {
                if !is_wildcard_origin {
                    let matches = self.find_fuzzy_matches(token);
                    if let Some((closest_match, distance)) = matches.into_iter().next() {
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
                        }
                    } else {
                        if processed_query_terms.len() == 1 {
                            return Vec::new();
                        }
                    }
                } else {
                }
            }
        }

        let mut intersection_results: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();
        for (doc_id, term_map) in candidate_docs {
            let mut all_terms_present = true;
            for (q_token_original, is_wildcard_origin) in processed_query_terms {
                let actual_term = if *is_wildcard_origin {
                    q_token_original
                } else {
                    fuzzy_matched_terms
                        .get(q_token_original)
                        .unwrap_or(q_token_original)
                };

                if !term_map.contains_key(actual_term) {
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
            let doc_len = self
                .documents
                .get(&doc_id)
                .map_or(0.0, |d| d.num_tokens as f64);

            for (q_token_original, is_wildcard_origin) in processed_query_terms {
                let actual_term = if *is_wildcard_origin {
                    q_token_original
                } else {
                    fuzzy_matched_terms
                        .get(q_token_original)
                        .unwrap_or(q_token_original)
                };

                let tf = term_frequencies_and_pos
                    .get(actual_term)
                    .map_or(0, |v| v.len()) as f64;

                if tf == 0.0 {
                    continue;
                }

                let num_docs_with_term = self.index.get(actual_term).map_or(0, |v| v.len()) as f64;

                let idf = ((self.total_docs as f64 - num_docs_with_term + 0.5)
                    / (num_docs_with_term + 0.5)
                    + 1.0)
                    .log10();

                let term_freq_comp = (tf * (BM25_K1 + 1.0))
                    / (tf
                        + BM25_K1
                            * (1.0 - BM25_B + BM25_B * (doc_len / self.avg_doc_length.max(1.0))));

                let mut term_score = idf * term_freq_comp;

                if !is_wildcard_origin && fuzzy_matched_terms.contains_key(q_token_original) {
                    term_score *= 0.5;
                }

                score += term_score;
            }
            ranked_results.push((score, doc_id));
        }

        ranked_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let terms_for_snippet_highlighting: Vec<String> = processed_query_terms
            .iter()
            .filter_map(|(token, is_wildcard_origin)| {
                if *is_wildcard_origin {
                    Some(token.clone())
                } else {
                    fuzzy_matched_terms
                        .get(token)
                        .cloned()
                        .or(Some(token.clone()))
                }
            })
            .collect();

        ranked_results
            .into_iter()
            .filter_map(|(score, doc_id)| {
                self.documents.get(&doc_id).cloned().map(|doc| {
                    let content_lower = doc.content.to_lowercase();

                    let mut first_match_idx = None;
                    for highlight_term in &terms_for_snippet_highlighting {
                        if let Some(idx) = content_lower.find(highlight_term) {
                            first_match_idx = Some(idx);
                            break;
                        }
                    }

                    let snippet = if let Some(start_char_idx) = first_match_idx {
                        let context_start = start_char_idx.saturating_sub(50);
                        let context_end =
                            (start_char_idx + terms_for_snippet_highlighting[0].len() + 50)
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

                        for term_to_highlight in &terms_for_snippet_highlighting {
                            let re_str = format!(r"(?i)\b{}\b", regex::escape(term_to_highlight));
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
                        doc: doc.clone(),
                        score,
                        snippet,
                        tags: doc.tags.clone(),
                    }
                })
            })
            .collect()
    }

    fn perform_phrase_search_and_rank(
        &self,
        phrase_query_text: &str,
        _original_query: &str,
    ) -> Vec<SearchResult> {
        let query_tokens_with_pos = crate::tokenizer::tokenize(phrase_query_text);

        if query_tokens_with_pos.is_empty() {
            return Vec::new();
        }

        let query_stemmed_tokens: Vec<String> = query_tokens_with_pos
            .iter()
            .map(|(s, _)| s.clone())
            .collect();

        let mut common_docs_data: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();

        for (token_idx, token) in query_stemmed_tokens.iter().enumerate() {
            if let Some(doc_entries) = self.index.get(token) {
                if token_idx == 0 {
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

        let terms_to_highlight_phrase: Vec<String> = query_stemmed_tokens.clone();

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

                        for term_to_highlight in &terms_to_highlight_phrase {
                            let re_str = format!(r"(?i)\b{}\b", regex::escape(term_to_highlight));
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
                        doc: doc.clone(),
                        score,
                        snippet,
                        tags: doc.tags.clone(),
                    }
                })
            })
            .collect()
    }

    // Helper function to extract text from a PDF file
    fn extract_text_from_pdf(path: &Path) -> Result<String> {
        let text = extract_text(path).context("Failed to extract text from PDF")?;
        Ok(text)
    }

    pub fn load_documents_from_directory(&mut self, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(anyhow!("Provided path is not a directory"));
        }

        let tag_regex = regex::Regex::new(r"#(\w+)").unwrap();

        let mut files_in_corpus: HashMap<PathBuf, u64> = HashMap::new();
        let mut document_paths_in_index: HashMap<PathBuf, u32> = HashMap::new();

        for (doc_id, doc) in &self.documents {
            document_paths_in_index.insert(doc.path.clone(), *doc_id);
        }

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();
            if file_path.is_file() {
                let extension = file_path.extension().and_then(|s| s.to_str());
                match extension {
                    Some("txt") | Some("md") | Some("html") | Some("pdf") => {
                        let metadata = fs::metadata(&file_path)?;
                        let modified_time_secs =
                            metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs();
                        files_in_corpus.insert(file_path, modified_time_secs);
                    }
                    _ => {
                        println!("Skipping unsupported file type: {:?}", file_path);
                    }
                }
            }
        }

        let mut docs_to_add_or_update_details: Vec<Document> = Vec::new();
        let mut doc_ids_to_remove: Vec<u32> = Vec::new();

        let mut current_doc_ids_in_corpus = HashMap::new();
        for (indexed_path, indexed_doc_id) in &document_paths_in_index {
            if !files_in_corpus.contains_key(indexed_path) {
                doc_ids_to_remove.push(*indexed_doc_id);
            } else {
                current_doc_ids_in_corpus.insert(indexed_path.clone(), *indexed_doc_id);
            }
        }

        for (file_path_owned, current_modified_time) in files_in_corpus {
            if let Some(existing_doc_id) = current_doc_ids_in_corpus.get(&file_path_owned) {
                if let Some(existing_doc) = self.documents.get(existing_doc_id) {
                    if existing_doc.modified_time != current_modified_time {
                        println!("Updating modified document: {:?}", file_path_owned);
                        doc_ids_to_remove.push(*existing_doc_id);

                        let content = match file_path_owned.extension().and_then(|ext| ext.to_str())
                        {
                            Some("txt") | Some("md") => fs::read_to_string(&file_path_owned)
                                .context("Failed to read text/markdown file")?,
                            Some("html") => {
                                let html_content = fs::read_to_string(&file_path_owned)
                                    .context("Failed to read HTML file")?;
                                Html::parse_document(&html_content)
                                    .select(&Selector::parse("body").unwrap())
                                    .next()
                                    .map(|element| element.text().collect::<String>())
                                    .unwrap_or_else(|| "".to_string())
                            }
                            Some("pdf") => Self::extract_text_from_pdf(&file_path_owned)?,
                            _ => Err(anyhow!(
                                "Unsupported file type for indexing: {:?}",
                                file_path_owned
                            ))?,
                        };
                        let extracted_tags = tag_regex
                            .captures_iter(&content)
                            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_lowercase()))
                            .collect();
                        let num_doc_tokens = crate::tokenizer::tokenize(&content).len();

                        docs_to_add_or_update_details.push(Document {
                            id: *existing_doc_id,
                            path: file_path_owned.clone(),
                            content,
                            title: file_path_owned
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            tags: extracted_tags,
                            num_tokens: num_doc_tokens,
                            modified_time: current_modified_time,
                        });
                    }
                }
            } else {
                println!("Adding new document: {:?}", file_path_owned);
                let content = match file_path_owned.extension().and_then(|ext| ext.to_str()) {
                    Some("txt") | Some("md") => fs::read_to_string(&file_path_owned)
                        .context("Failed to read text/markdown file")?,
                    Some("html") => {
                        let html_content = fs::read_to_string(&file_path_owned)
                            .context("Failed to read HTML file")?;
                        Html::parse_document(&html_content)
                            .select(&Selector::parse("body").unwrap())
                            .next()
                            .map(|element| element.text().collect::<String>())
                            .unwrap_or_else(|| "".to_string())
                    }
                    Some("pdf") => Self::extract_text_from_pdf(&file_path_owned)?,
                    _ => Err(anyhow!(
                        "Unsupported file type for indexing: {:?}",
                        file_path_owned
                    ))?,
                };
                let extracted_tags = tag_regex
                    .captures_iter(&content)
                    .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_lowercase()))
                    .collect();
                let num_doc_tokens = crate::tokenizer::tokenize(&content).len();

                let new_doc_id = self.next_doc_id.fetch_add(1, Ordering::SeqCst);
                docs_to_add_or_update_details.push(Document {
                    id: new_doc_id,
                    path: file_path_owned.clone(),
                    content,
                    title: file_path_owned
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    tags: extracted_tags,
                    num_tokens: num_doc_tokens,
                    modified_time: current_modified_time,
                });
            }
        }

        for doc_id in doc_ids_to_remove {
            self.remove_document(doc_id);
        }

        for doc_details in docs_to_add_or_update_details {
            self.add_document(doc_details);
        }

        self.total_docs = self.documents.len();
        let mut total_tokens: usize = 0;
        for doc in self.documents.values() {
            total_tokens += doc.num_tokens;
        }

        if self.total_docs > 0 {
            self.avg_doc_length = total_tokens as f64 / self.total_docs as f64;
        } else {
            self.avg_doc_length = 0.0;
        }

        self.clear_cache();
        Ok(())
    }

    pub fn total_documents(&self) -> usize {
        self.total_docs
    }

    pub fn generate_network_graph_data(&self) -> Result<String> {
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut searchable_documents: HashMap<u32, ClientSearchableDocument> = HashMap::new();
        let mut processed_edges: std::collections::HashSet<(u32, u32)> =
            std::collections::HashSet::new();

        for doc in self.documents.values() {
            let mut content_preview = doc.content.chars().take(300).collect::<String>();
            if doc.content.len() > 300 {
                content_preview.push_str("...");
            }

            let file_extension = doc
                .path
                .extension()
                .and_then(|os_str| os_str.to_str())
                .unwrap_or("unknown")
                .to_string();
            nodes.push(GraphNode {
                id: doc.id,
                label: doc.title.clone(),
                title: format!("{} (Tags: {})", doc.title, doc.tags.join(", ")),
                group: file_extension,
                content_preview: content_preview.clone(), // Clone for graph node
                js_tags: doc.tags.clone(),
            });

            // Populate searchable_documents map
            searchable_documents.insert(
                doc.id,
                ClientSearchableDocument {
                    id: doc.id,
                    title: doc.title.clone(),
                    content: doc.content.clone(),
                    tags: doc.tags.clone(),
                    content_preview,
                },
            );

            for other_doc in self.documents.values() {
                if doc.id == other_doc.id {
                    continue;
                }

                let mut shared_tags_count = 0;
                for tag in &doc.tags {
                    if other_doc.tags.contains(tag) {
                        shared_tags_count += 1;
                    }
                }

                if shared_tags_count > 0 {
                    let (node1, node2) = if doc.id < other_doc.id {
                        (doc.id, other_doc.id)
                    } else {
                        (other_doc.id, doc.id)
                    };

                    if processed_edges.insert((node1, node2)) {
                        edges.push(GraphEdge {
                            from: node1,
                            to: node2,
                            width: shared_tags_count as f64,
                        });
                    }
                }
            }
        }

        let full_app_data = FullWebAppData {
            nodes,
            edges,
            searchable_documents,
        };
        let json_string = serde_json::to_string_pretty(&full_app_data)
            .context("Failed to serialize full app data to JSON")?;

        Ok(json_string)
    }
}
