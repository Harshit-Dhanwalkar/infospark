// src/main.rs
mod inverted_index;
mod tokenizer;

use inverted_index::{InvertedIndex, SearchResult};
use std::fs;
use std::path::Path;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use anyhow::{Context, Result, anyhow};
use colored::*;

const INDEX_FILE: &str = "search_index.bin";
const HISTORY_FILE: &str = ".infospark_history";
const GRAPH_HTML_FILE: &str = "infospark_graph.html";

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
        let readline =
            rl.readline("Enter search query (or 'graph' to open web app, 'exit' to quit): ");

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
                } else if query.eq_ignore_ascii_case("graph") {
                    println!("Generating interactive web app data...");
                    match index.generate_network_graph_data() {
                        Ok(json_data) => {
                            let escaped_json_data = json_data
                                .replace("\\", "\\\\") // Escape backslashes
                                .replace("\"", "\\\"") // Escape double quotes
                                .replace("\n", "\\n") // Escape newlines
                                .replace("\r", "\\r") // Escape carriage returns
                                .replace("\t", "\\t") // Escape tabs
                                .replace("`", "\\`"); // Escape backticks for JS template literal

                            let html_content = format!(
                                r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Infospark Interactive Graph & Search</title>
    <script type="text/javascript" src="https://unpkg.com/vis-network@9.1.2/dist/vis-network.min.js"></script>
    <link href="https://unpkg.com/vis-network@9.1.2/dist/vis-network.min.css" rel="stylesheet" type="text/css" />
    <style type="text/css">
        @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap');
        body {{
            font-family: 'Inter', sans-serif;
            margin: 0;
            padding: 0;
            display: flex; /* Use flexbox for layout */
            height: 100vh; /* Full viewport height */
            overflow: hidden;
            background-color: #f0f2f5;
        }}
        #sidebar {{
            width: 300px; /* Fixed width sidebar */
            background-color: #fff;
            box-shadow: 2px 0 5px rgba(0,0,0,0.1);
            display: flex;
            flex-direction: column;
            padding: 15px;
            overflow-y: auto; /* Scroll for content */
            z-index: 101; /* Above graph */
        }}
        #main-content {{
            flex-grow: 1; /* Graph takes remaining space */
            position: relative;
        }}
        #mynetwork {{
            width: 100%;
            height: 100%;
            border: 1px solid lightgray;
            background-color: #f9f9f9;
        }}
        #search-container {{
            margin-bottom: 20px;
            padding-bottom: 15px;
            border-bottom: 1px solid #eee;
        }}
        #search-input {{
            width: calc(100% - 20px);
            padding: 10px;
            margin-bottom: 10px;
            border: 1px solid #ddd;
            border-radius: 5px;
            font-size: 1em;
        }}
        .search-button {{
            padding: 8px 12px;
            background-color: #007bff;
            color: white;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 0.9em;
            margin-right: 5px;
            transition: background-color 0.2s ease;
        }}
        .search-button:hover {{
            background-color: #0056b3;
        }}
        #reset-search-button {{
            background-color: #6c757d;
        }}
        #reset-search-button:hover {{
            background-color: #5a6268;
        }}
        #search-results {{
            flex-grow: 1;
            overflow-y: auto;
            border-top: 1px solid #eee;
            padding-top: 15px;
        }}
        .search-result-item {{
            background-color: #f8f9fa;
            border: 1px solid #e9ecef;
            border-radius: 5px;
            padding: 10px;
            margin-bottom: 10px;
            cursor: pointer;
            transition: background-color 0.2s ease;
        }}
        .search-result-item:hover {{
            background-color: #e2e6ea;
        }}
        .search-result-item h4 {{
            margin-top: 0;
            margin-bottom: 5px;
            color: #333;
        }}
        .search-result-item p {{
            font-size: 0.9em;
            color: #666;
            margin-bottom: 5px;
        }}
        .search-result-item .tags {{
            font-size: 0.8em;
            color: #00796b;
        }}
        .search-result-item .tags span {{
            background-color: #e0f7fa;
            padding: 2px 6px;
            border-radius: 3px;
            margin-right: 3px;
            display: inline-block;
            margin-bottom: 3px;
        }}

        /* Graph filter controls */
        #graph-filter-controls {{
            position: absolute;
            top: 10px;
            right: 10px;
            background: rgba(255, 255, 255, 0.9);
            padding: 10px 15px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            display: flex;
            gap: 10px;
            align-items: center;
            z-index: 100;
        }}
        #graph-filter-input {{
            padding: 8px;
            border: 1px solid #ccc;
            border-radius: 5px;
            font-size: 0.9em;
            width: 180px;
        }}
        .graph-filter-button {{
            padding: 8px 12px;
            background-color: #4CAF50;
            color: white;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 0.9em;
            transition: background-color 0.2s ease;
        }}
        .graph-filter-button:hover {{
            background-color: #45a049;
        }}
        #reset-graph-filter-button {{
            background-color: #008CBA;
        }}
        #reset-graph-filter-button:hover {{
            background-color: #007bb5;
        }}

        .vis-tooltip {{
            background-color: #333;
            color: white;
            padding: 8px 12px;
            border-radius: 5px;
            font-size: 14px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.2);
            max-width: 300px;
            word-wrap: break-word;
        }}
        .modal-overlay {{
            position: fixed;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background: rgba(0, 0, 0, 0.6);
            display: flex;
            justify-content: center;
            align-items: center;
            z-index: 1000;
            visibility: hidden;
            opacity: 0;
            transition: visibility 0s, opacity 0.3s ease;
        }}
        .modal-overlay.visible {{
            visibility: visible;
            opacity: 1;
        }}
        .modal-content {{
            background: white;
            padding: 30px;
            border-radius: 10px;
            box-shadow: 0 5px 20px rgba(0, 0, 0, 0.3);
            width: 80%;
            max-width: 600px;
            max-height: 80vh;
            overflow-y: auto;
            position: relative;
        }}
        .modal-header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid #eee;
            padding-bottom: 15px;
            margin-bottom: 15px;
        }}
        .modal-header h3 {{
            margin: 0;
            color: #333;
            font-size: 1.5em;
        }}
        .modal-close-button {{
            background: #f44336;
            color: white;
            border: none;
            border-radius: 50%;
            width: 30px;
            height: 30px;
            font-size: 1.2em;
            cursor: pointer;
            display: flex;
            justify-content: center;
            align-items: center;
            transition: background-color 0.2s ease;
        }}
        .modal-close-button:hover {{
            background-color: #d32f2f;
        }}
        .modal-body p {{
            font-size: 0.95em;
            line-height: 1.6;
            color: #555;
            white-space: pre-wrap;
        }}
        .modal-tags {{
            margin-top: 10px;
            font-size: 0.85em;
            color: #666;
        }}
        .modal-tags span {{
            background-color: #e0f7fa;
            color: #00796b;
            padding: 3px 8px;
            border-radius: 5px;
            margin-right: 5px;
            display: inline-block;
            margin-bottom: 5px;
        }}
    </style>
</head>
<body>
    <div id="sidebar">
        <div id="search-container">
            <h3>Document Search</h3>
            <input type="text" id="search-input-text" placeholder="Search documents...">
            <button id="perform-search-button" class="search-button">Search</button>
            <button id="clear-search-button" class="search-button">Clear Results</button>
        </div>
        <div id="search-results">
            <p style="color: #777;">Type a query and click 'Search' or hit Enter.</p>
        </div>
    </div>
    <div id="main-content">
        <div id="mynetwork"></div>
        <div id="graph-filter-controls">
            <input type="text" id="graph-filter-input" placeholder="Filter graph by tag or keyword...">
            <button id="graph-filter-tag-button" class="graph-filter-button">Filter by Tag</button>
            <button id="graph-filter-keyword-button" class="graph-filter-button">Filter by Keyword</button>
            <button id="reset-graph-filter-button" class="graph-filter-button">Reset Graph</button>
        </div>
    </div>

    <!-- Document Preview Modal -->
    <div id="documentModal" class="modal-overlay">
        <div class="modal-content">
            <div class="modal-header">
                <h3 id="modalTitle"></h3>
                <button id="modalCloseButton" class="modal-close-button">&times;</button>
            </div>
            <div class="modal-body">
                <p id="modalContent"></p>
                <div id="modalTags" class="modal-tags"></div>
            </div>
        </div>
    </div>

    <script type="text/javascript">
        console.log("Vis object after script load:", typeof vis !== 'undefined' ? vis : "vis not defined yet.");

        const fullAppDataJson = `{}`;

        let originalNodes = new vis.DataSet([]);
        let originalEdges = new vis.DataSet([]);
        let searchableDocuments = {{}};
        let network;

        try {{
            const parsedData = JSON.parse(fullAppDataJson);
            console.log("Parsed Full App Data from Rust:", parsedData);
            originalNodes = new vis.DataSet(parsedData.nodes);
            originalEdges = new vis.DataSet(parsedData.edges);
            searchableDocuments = parsedData.searchable_documents;
        }} catch (e) {{
            console.error("Error parsing full app data:", e);
            console.error("Data was likely malformed. Please check backend generation or content of fullAppDataJson."); 
            document.body.innerHTML = '<div style="text-align: center; padding-top: 50px; color: #777;">Error loading application data. Check browser console for details.</div>';
        }}

        const container = document.getElementById('mynetwork');
        const data = {{ nodes: originalNodes, edges: originalEdges }};
        const options = {{
            nodes: {{
                shape: 'dot',
                size: 16,
                font: {{
                    size: 12,
                    color: '#333'
                }},
                borderWidth: 2,
                shadow:true
            }},
            edges: {{
                width: 1,
                shadow:true,
                color: {{
                    color: '#848484',
                    highlight: '#848484',
                    hover: '#848484',
                    inherit: 'from',
                    opacity: 0.5
                }}
            }},
            groups: {{
                txt: {{ color: {{ background: '#ADD8E6', border: '#4682B4' }} }},
                md: {{ color: {{ background: '#90EE90', border: '#3CB371' }} }},
                html: {{ color: {{ background: '#FFDAB9', border: '#FF8C00' }} }},
                pdf: {{ color: {{ background: '#FFB6C1', border: '#DC143C' }} }},
                unknown: {{ color: {{ background: '#D3D3D3', border: '#696969' }} }}
            }},
            physics: {{
                enabled: true,
                barnesHut: {{
                    gravitationalConstant: -2000,
                    centralGravity: 0.3,
                    springLength: 95,
                    springConstant: 0.04,
                    damping: 0.09,
                    avoidOverlap: 0
                }},
                solver: 'barnesHut',
                stabilization: {{
                    iterations: 2500
                }}
            }},
            interaction: {{
                hover: true,
                navigationButtons: true,
                keyboard: true
            }}
        }};

        // Initialize network only if nodes are properly initialized
        if (originalNodes.length > 0) {{
            network = new vis.Network(container, data, options);

            network.on("doubleClick", function (params) {{
                if (params.nodes.length > 0) {{
                    const nodeId = params.nodes[0];
                    const node = originalNodes.get(nodeId); 

                    const modal = document.getElementById('documentModal');
                    const modalTitle = document.getElementById('modalTitle');
                    const modalContent = document.getElementById('modalContent');
                    const modalTags = document.getElementById('modalTags');

                    modalTitle.textContent = node.label; 
                    modalContent.textContent = node.content_preview;

                    modalTags.innerHTML = ''; 
                    if (node.js_tags && node.js_tags.length > 0) {{
                        node.js_tags.forEach(tag => {{
                            const tagSpan = document.createElement('span');
                            tagSpan.textContent = `#${{tag}}`; 
                            modalTags.appendChild(tagSpan);
                        }});
                    }}

                    modal.classList.add('visible');
                }}
            }});
        }} else {{
            console.warn("No nodes to display. Graph will be empty.");
            document.getElementById('mynetwork').innerHTML = '<div style="text-align: center; padding-top: 50px; color: #777;">No graph data to display. Please ensure your corpus has documents and/or tags.</div>';
        }}

        document.getElementById('modalCloseButton').addEventListener('click', function() {{
            document.getElementById('documentModal').classList.remove('visible');
        }});

        document.getElementById('documentModal').addEventListener('click', function(event) {{
            if (event.target === this) {{ 
                this.classList.remove('visible');
            }}
        }});


        // ----- Client-Side Search Logic -----
        const searchInputText = document.getElementById('search-input-text');
        const performSearchButton = document.getElementById('perform-search-button');
        const clearSearchButton = document.getElementById('clear-search-button');
        const searchResultsDiv = document.getElementById('search-results');

        // Simple tokenizer for client-side search (JS version)
        function tokenize(text) {{
            return text.toLowerCase().match(/\b\w+\b/g) || [];
        }}

        function displaySearchResults(results) {{
            searchResultsDiv.innerHTML = '';
            if (results.length === 0) {{
                searchResultsDiv.innerHTML = '<p style="color: #777;">No documents found matching your search.</p>';
                return;
            }}

            results.forEach(doc => {{
                const item = document.createElement('div');
                item.className = 'search-result-item';
                item.onclick = () => {{
                    // Highlight node on graph when clicking search result
                    network.selectNodes([doc.id]);
                    network.focus(doc.id, {{scale: 1.5, animation: {{duration: 500, easingFunction: "easeOutCubic"}} }});
                    // Show modal preview
                    const node = originalNodes.get(doc.id);
                    if (node) {{
                        document.getElementById('modalTitle').textContent = node.label; 
                        document.getElementById('modalContent').textContent = node.content_preview; 
                        const modalTags = document.getElementById('modalTags');
                        modalTags.innerHTML = ''; 
                        if (node.js_tags && node.js_tags.length > 0) {{
                            node.js_tags.forEach(tag => {{
                                const tagSpan = document.createElement('span');
                                tagSpan.textContent = `#${{tag}}`;
                                modalTags.appendChild(tagSpan);
                            }});
                        }}
                        document.getElementById('documentModal').classList.add('visible');
                    }}
                }};

                const titleElem = document.createElement('h4');
                titleElem.textContent = doc.title;
                item.appendChild(titleElem);

                const previewElem = document.createElement('p');
                previewElem.textContent = doc.content_preview;
                item.appendChild(previewElem);

                if (doc.tags && doc.tags.length > 0) {{
                    const tagsElem = document.createElement('div');
                    tagsElem.className = 'tags';
                    doc.tags.forEach(tag => {{
                        const tagSpan = document.createElement('span');
                        tagSpan.textContent = `#${{tag}}`;
                        tagsElem.appendChild(tagSpan);
                    }});
                    item.appendChild(tagsElem);
                }}
                searchResultsDiv.appendChild(item);
            }});
        }}

        function performClientSideSearch() {{
            const query = searchInputText.value.toLowerCase().trim();
            const results = [];
            const queryTokens = tokenize(query);

            if (query === "") {{
                displaySearchResults([]);
                filterGraphByNodeIds([]);
                return;
            }}

            let filteredNodeIds = new Set();

            for (const docId in searchableDocuments) {{
                const doc = searchableDocuments[docId];
                let isMatch = false;

                // Tag Search (starts with #)
                if (query.startsWith('#')) {{
                    const tagQuery = query.substring(1);
                    if (doc.tags && doc.tags.some(tag => tag.includes(tagQuery))) {{
                        isMatch = true;
                    }}
                }} 
                // Keyword/General Search
                else {{
                    const docContentTokens = tokenize(doc.content);
                    const docTitleTokens = tokenize(doc.title);

                    for (const qToken of queryTokens) {{
                        // Basic keyword match in content or title
                        if (docContentTokens.includes(qToken) || docTitleTokens.includes(qToken)) {{
                            isMatch = true;
                            break;
                        }}
                        // Simple wildcard match (ends with *)
                        if (qToken.endsWith('*') && qToken.length > 1) {{
                            const prefix = qToken.slice(0, -1);
                            if (docContentTokens.some(dToken => dToken.startsWith(prefix)) || 
                                docTitleTokens.some(dToken => dToken.startsWith(prefix))) {{
                                isMatch = true;
                                break;
                            }}
                        }}
                        // Fuzzy search (very basic, just check if query is substring)
                        if (doc.content.toLowerCase().includes(query) || doc.title.toLowerCase().includes(query)) {{
                            isMatch = true;
                            break;
                        }}
                    }}
                }}

                if (isMatch) {{
                    results.push(doc);
                    filteredNodeIds.add(doc.id);
                }}
            }}
            displaySearchResults(results);
            filterGraphByNodeIds(Array.from(filteredNodeIds));
        }}

        function clearClientSideSearch() {{
            searchInputText.value = '';
            displaySearchResults([]);
            filterGraphByNodeIds([]);
        }}

        performSearchButton.addEventListener('click', performClientSideSearch);
        clearSearchButton.addEventListener('click', clearClientSideSearch);
        searchInputText.addEventListener('keypress', (e) => {{
            if (e.key === 'Enter') {{
                performClientSideSearch();
            }}
        }});

        // ----- Graph Filtering Controls -----
        const graphFilterInput = document.getElementById('graph-filter-input');
        const graphFilterTagButton = document.getElementById('graph-filter-tag-button');
        const graphFilterKeywordButton = document.getElementById('graph-filter-keyword-button');
        const resetGraphFilterButton = document.getElementById('reset-graph-filter-button');

        function filterGraphByNodeIds(nodeIdsToShow) {{
            if (network) {{
                if (nodeIdsToShow.length === 0) {{
                    // If no IDs to show, display all original nodes/edges
                    network.setData({{
                        nodes: originalNodes,
                        edges: originalEdges
                    }});
                }} else {{
                    // Filter nodes: only include those in nodeIdsToShow
                    const filteredNodes = originalNodes.get({{
                        filter: function (node) {{
                            return nodeIdsToShow.includes(node.id);
                        }}
                    }});

                    // Filter edges: only include edges where BOTH connected nodes are visible
                    const visibleNodeIdsSet = new Set(nodeIdsToShow);
                    const filteredEdges = originalEdges.get({{
                        filter: function (edge) {{
                            return visibleNodeIdsSet.has(edge.from) && visibleNodeIdsSet.has(edge.to);
                        }}
                    }});

                    network.setData({{
                        nodes: new vis.DataSet(filteredNodes),
                        edges: new vis.DataSet(filteredEdges)
                    }});
                }}
                network.fit();
            }}
        }}

        // Combined graph filter logic
        function applyGraphFilter(filterType) {{
            const query = graphFilterInput.value.toLowerCase().trim();
            let nodesMatchingFilter = new Set();

            if (!query) {{
                filterGraphByNodeIds([]);
                return;
            }}

            originalNodes.forEach(node => {{
                let isMatch = false;
                if (filterType === 'tag') {{
                    if (node.js_tags && node.js_tags.some(tag => tag.includes(query))) {{
                        isMatch = true;
                    }}
                }} else if (filterType === 'keyword') {{
                    if (node.label.toLowerCase().includes(query) || node.content_preview.toLowerCase().includes(query)) {{
                        isMatch = true;
                    }}
                }}
                if (isMatch) {{
                    nodesMatchingFilter.add(node.id);
                }}
            }});
            filterGraphByNodeIds(Array.from(nodesMatchingFilter));
        }}

        function resetGraphFilter() {{
            graphFilterInput.value = '';
            filterGraphByNodeIds([]);
        }}

        graphFilterTagButton.addEventListener('click', () => applyGraphFilter('tag'));
        graphFilterKeywordButton.addEventListener('click', () => applyGraphFilter('keyword'));
        resetGraphFilterButton.addEventListener('click', resetGraphFilter);

        graphFilterInput.addEventListener('keypress', (e) => {{
            if (e.key === 'Enter') {{
                applyGraphFilter('keyword');
            }}
        }});
    </script>
</body>
</html>"#,
                                escaped_json_data
                            );

                            fs::write(GRAPH_HTML_FILE, html_content)
                                .context("Failed to write graph HTML file")?;

                            match open::that(GRAPH_HTML_FILE) {
                                Ok(_) => println!(
                                    "Automatically opened '{}' in your default web browser.",
                                    GRAPH_HTML_FILE.blue()
                                ),
                                Err(e) => eprintln!(
                                    "Failed to automatically open '{}': {:?}",
                                    GRAPH_HTML_FILE, e
                                ),
                            }
                        }
                        Err(e) => {
                            eprintln!("Error generating web app data: {:?}", e);
                        }
                    }
                } else {
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
                            if !result.tags.is_empty() {
                                let formatted_tags: Vec<String> = result
                                    .tags
                                    .iter()
                                    .map(|tag| format!("#{}", tag).blue().to_string())
                                    .collect();
                                println!("    - Tags: {}", formatted_tags.join(", "));
                            }
                            println!("    - Path: {:?}", result.doc.path);
                            println!("    - Snippet: {}\n", result.snippet);
                        }
                    }
                    println!("");
                }
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
