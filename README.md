# Infospark

Infospark is a high-performance, in-memory full-text search engine developed in Rust. It's designed for efficient indexing and retrieval of information from a collection of text documents, providing fast and relevant search results.

## Features

Infospark currently includes the following functionalities:

- **Fast In-memory Indexing:** Efficiently processes and stores document data in an inverted index structure.
- **Persistence:** Automatically saves the generated index to `search_index.bin` and loads it on subsequent runs, avoiding redundant indexing.
- **Tokenization & Normalization:** Processes text by tokenizing, lowercasing, filtering stop words, and applying stemming to ensure robust search matches.
- **Keyword Search:** Supports basic keyword queries with relevance ranking using a TF-IDF based scoring mechanism.
- **Positional Indexing (Phase 1 of Phrase Search):** The groundwork for phrase search is laid by storing word positions within documents, allowing for accurate phrase matching in future updates.
- **Highlighted Snippets:** Provides contextual snippets in search results with query terms highlighted for easy readability.
- **Parallel Document Indexing:** Utilizes Rust's concurrency features (via `rayon`) to speed up the initial document loading and indexing process.
- **Search Result Caching (LRU):** Employs a Least Recently Used (LRU) cache to store and quickly retrieve results for frequent queries.

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
    After indexing/loading, you will be prompted to enter search queries. Type your query and press Enter. Type `exit` to quit the application.

## Planned Future Enhancements

We are continually looking to improve Infospark. Here are some features planned for future development:

- **Full Phrase Search Logic:** Implement the complete algorithm for matching exact phrases in queries (e.g., `"rust programming"`).
- **Boolean Operators:** Introduce support for `AND`, `OR`, and `NOT` operators in queries.
- **Fuzzy Matching / Typo Tolerance:** Improve search resilience to misspellings and typos.
- **More Advanced Ranking:** Integrate sophisticated ranking algorithms like Okapi BM25 for even more relevant search results.
- **Field-Specific Search:** Allow users to limit searches to specific document fields (e.g., `title:query`).
- **Support for More File Types:** Expand indexing capabilities beyond `.txt` to include formats like Markdown (`.md`), HTML (`.html`), PDF (`.pdf`), and more.
- **Incremental Indexing:** Develop a mechanism to only re-index new or changed documents, significantly reducing indexing time for updated corpora.
- **Document Deletion / Updates:** Add functionality to dynamically remove or update documents within the existing index.
- **Configurable Paths:** Allow users to specify custom paths for the corpus directory and index file.

## Contributing

Contributions are welcome! Feel free to open issues or pull requests on the GitHub repository.

## License

[MIT License](LICENSE)
