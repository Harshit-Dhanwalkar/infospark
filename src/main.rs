// src/main.rs
mod inverted_index;
mod tokenizer;

use inverted_index::{InvertedIndex, SearchResult};
use std::fs;
use std::path::Path;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use anyhow::{Context, Result};

const INDEX_FILE: &str = "search_index.bin";
const HISTORY_FILE: &str = ".infospark_history";

fn main() -> Result<()> {
    let mut index = InvertedIndex::new();
    let index_path = Path::new(INDEX_FILE);

    let mut rl = DefaultEditor::new().context("Failed to create readline editor")?;

    if rl.load_history(HISTORY_FILE).is_err() {
        println!("No previous search history found.");
    }

    if index_path.exists() {
        println!("Loading existing index from '{}'...", INDEX_FILE);
        let encoded_data = fs::read(index_path).context("Failed to read existing index file")?;

        index = InvertedIndex::from_serialized_data(&encoded_data)
            .context("Failed to deserialize existing index")?;

        println!(
            "Index loaded. Total documents indexed: {}\n",
            index.total_documents()
        );
    } else {
        let corpus_path = Path::new("corpus");
        println!(
            "No existing index found. Loading documents from: {:?}\n",
            corpus_path
        );
        index
            .load_documents_from_directory(corpus_path)
            .context("Failed to load documents from directory")?;
        println!(
            "\nIndexing complete. Total documents indexed: {}\n",
            index.total_documents()
        );

        println!("Saving index to '{}'...", INDEX_FILE);
        let encoded_data = index
            .to_serialized_data()
            .context("Failed to serialize index for saving")?;
        fs::write(index_path, encoded_data).context("Failed to write index to file")?;
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

                rl.add_history_entry(line.as_str())
                    .context("Failed to add query to history")?;

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
                eprintln!("Error reading line: {:?}", err);
                return Err(anyhow::Error::new(err).context("Error during readline operation"));
            }
        }
    }

    rl.save_history(HISTORY_FILE)
        .context("Failed to save history file")?;

    Ok(())
}
