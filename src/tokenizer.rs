// src/tokenizer.rs
use rust_stemmers::{Algorithm, Stemmer}; // Import for stemming
use std::collections::HashSet;
use stop_words::{LANGUAGE, get}; // Import for stop words

// Initialize stop words set once (e.g., as a lazy static or in a constructor)
// For simplicity in a function, we'll create it each time for now,
// but for performance, you'd want to initialize it once.
lazy_static::lazy_static! {
    static ref STOP_WORDS: HashSet<String> = get(LANGUAGE::English).into_iter().collect();
}

pub fn tokenize(text: &str) -> Vec<String> {
    let en_stemmer = Stemmer::create(Algorithm::English); // Create English stemmer

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric()) // Split by anything that's not alphanumeric
        .filter(|s| !s.is_empty()) // Remove empty strings from consecutive delimiters
        .map(|s| s.to_string())
        .filter(|s| !STOP_WORDS.contains(s)) // Filter out stop words
        .map(|s| en_stemmer.stem(&s).to_string()) // Apply stemming
        .collect()
}
