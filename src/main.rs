// src/main.rs

mod inverted_index;
mod tokenizer;

use inverted_index::{InvertedIndex, SearchResult};
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RlResult};

const INDEX_FILE: &str = "search_index.bin";
const HISTORY_FILE: &str = ".infospark_history";

fn main() -> Result<(), Box<dyn Error>> {
    let mut index = InvertedIndex::new();
    let index_path = Path::new(INDEX_FILE);

    let mut rl = DefaultEditor::new()?;

    if rl.load_history(HISTORY_FILE).is_err() {
        println!("No previous search history found.");
    }

    // Try to load existing index
    if index_path.exists() {
        println!("Loading existing index from '{}'...", INDEX_FILE);
        let encoded_data = fs::read(index_path)?;
        index = InvertedIndex::from_serialized_data(&encoded_data)?;
        println!(
            "Index loaded. Total documents indexed: {}\n",
            index.total_documents()
        );
    } else {
        // If no index exists, build it from corpus
        let corpus_path = Path::new("corpus");
        println!(
            "No existing index found. Loading documents from: {:?}\n",
            corpus_path
        );
        index.load_documents_from_directory(corpus_path)?;
        println!(
            "\nIndexing complete. Total documents indexed: {}\n",
            index.total_documents()
        );

        // Save the newly built index
        println!("Saving index to '{}'...", INDEX_FILE);
        let encoded_data = index.to_serialized_data()?;
        fs::write(index_path, encoded_data)?;
        println!("Index saved.\n");
    }

    loop {
        let readline = rl.readline("Enter search query (or 'exit' to quit): ");

        match readline {
            Ok(line) => {
                let query = line.trim();

                if query.is_empty() {
                    continue;
                }

                rl.add_history_entry(line.as_str())?;

                if query.eq_ignore_ascii_case("exit") {
                    break;
                }

                let results: Vec<SearchResult> = index.search(query);

                if results.is_empty() {
                    println!("No results found for '{}'", query);
                } else {
                    println!("Results for '{}':", query);
                    for result in results {
                        println!(
                            "  - Doc ID: {}, Title: {:?}, Score: {:.4}",
                            result.doc.id, result.doc.title, result.score
                        );
                        println!("    Snippet: {}", result.snippet);
                        println!("    Path: {:?}\n", result.doc.path);
                    }
                }
                println!("");
            }
            Err(ReadlineError::Interrupted) => {
                println!("\nCtrl-C received. Exiting.");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("\nCtrl-D received. Exiting.");
                break;
            }
            Err(err) => {
                println!("Error reading line: {:?}", err);
                break;
            }
        }
    }

    rl.save_history(HISTORY_FILE)?;

    Ok(())
}
