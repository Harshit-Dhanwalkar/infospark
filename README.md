# Infospark

Infospark is a high-performance, in-memory full-text search engine developed in Rust. It's designed for efficient indexing and retrieval of information from a collection of text documents, providing fast and relevant search results.

## Features

Infospark currently includes the following functionalities:

- **Fast In-memory Indexing:** Efficiently processes and stores document data in an inverted index structure.
- **Persistence:** Automatically saves the generated index to `search_index.bin` and loads it on subsequent runs, avoiding redundant indexing.
- **Tokenization & Normalization:** Tokenization & Normalization: Processes text by tokenizing, lowercasing, filtering stop words, and applying stemming to ensure robust search matches.
- **Keyword Search (BM25 Ranked):** Supports basic keyword queries with advanced relevance ranking using the `Okapi BM25 algorithm`, providing more accurate and nuanced results.
- **Full Phrase Search:** Accurately matches exact phrases in queries enclosed in double quotes (e.g., "rust programming").
- **Fuzzy Matching / Typo Tolerance:** Provides approximate matching for misspelled single-word queries, offering suggestions and results for terms close to your input (e.g., 'rst' for 'rust').
- **Wildcard / Prefix Search:** Supports wildcard queries using an asterisk (`*`) at the end of a word (e.g., `rust*` matches "rust", "rusty", "rusting"; `program*` matches "programming", etc.).
- **Highlighted Snippets:** Provides contextual snippets in search results with query terms highlighted for easy readability.
- **Parallel Document Indexing:** Utilizes Rust's concurrency features (via `rayon`) to speed up the initial document loading and indexing process.
- **Search Result Caching (LRU):** Employs a Least Recently Used (LRU) cache to store and quickly retrieve results for frequent queries.
- **Multi-format Document Support**: Indexes and searches across plain text (`.txt`), Markdown (`.md`), HTML (`.html`), and PDF (`.pdf`) documents.

## Getting Started

### Prerequisites

- **Rust:** Ensure you have Rust and Cargo installed. You can install them via `rustup`:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf [https://sh.rustup.rs](https://sh.rustup.rs) | sh
  ```
- **Project Structure:** Your documents should be `.txt` files placed in a directory named `corpus/` in the root of your project.

### Installation and Running

1.  **Clone the repository:**
    ```bash
    git clone [https://github.com/your-username/infospark.git](https://github.com/your-username/infospark.git) # Replace with your repo URL
    cd infospark
    ```
2.  **Prepare your corpus:**
    Create a `corpus/` directory inside the `infospark` project folder, and place your `.txt` documents there.
    ```
    infospark/
    ├── src/
    ├── corpus/
    │   ├── doc_a.txt
    │   ├── doc_b.txt
    │   ├── doc_c.txt
    │   ├── doc_d.md
    │   ├── doc_e.html
    │   ├── doc_f.pdf
    │   └── ...
    ├── Cargo.toml
    └── Cargo.lock
    ```
3.  **Run the application:**

    ```bash
    cargo run
    ```

    - **First Run:** The program will detect no existing index, index your documents from the `corpus/` directory, and then save the index to `search_index.bin`.
    - **Subsequent Runs:** The program will quickly load the existing `search_index.bin` file, saving the re-indexing time.

4.  **Interact:**
    After indexing/loading, you will be prompted to enter search queries. Type your query and press Enter. You can use:

        - Keywords: `rust` `language`
        - Exact Phrases: `"modern programming"`
        - Wildcard Terms: `program*`
        - Fuzzy Terms: `rst` (for `rust`)
        - Tags: `#rust`

    Type `exit` to quit the application.

## Contributing

Contributions are welcome! Feel free to open issues or pull requests on the GitHub repository.

## License

[MIT License](LICENSE)
