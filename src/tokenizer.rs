// src/tokenizer.rs
use rust_stemmers::{Algorithm, Stemmer};
use std::collections::HashSet;
use stop_words::{LANGUAGE, get};

lazy_static::lazy_static! {
    static ref STOP_WORDS: HashSet<String> = get(LANGUAGE::English).into_iter().collect();
}

pub fn tokenize(text: &str) -> Vec<(String, usize)> {
    let en_stemmer = Stemmer::create(Algorithm::English);
    let mut tokens_with_positions = Vec::new();
    let mut current_word_index = 0;

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .for_each(|s| {
            let token_string = s.to_string();
            if !STOP_WORDS.contains(&token_string) {
                let stemmed_token = en_stemmer.stem(&token_string).to_string();
                tokens_with_positions.push((stemmed_token, current_word_index));
                current_word_index += 1; // Increment position for the next valid word
            }
        });
    tokens_with_positions
}
