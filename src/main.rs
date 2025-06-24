// src/main.rs
mod inverted_index;
mod tokenizer;

use inverted_index::{InvertedIndex, SearchResult};
use std::fs;
use std::path::Path;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use anyhow::{Context, Result};
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
            rl.readline("Enter search query (or 'graph' to visualize, 'exit' to quit): ");

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
                    println!("Generating network graph data...");
                    match index.generate_network_graph_data() {
                        Ok(json_data) => {
                            let escaped_json_data = json_data
                                .replace("\\", "\\\\")
                                .replace("\"", "\\\"")
                                .replace("\n", "\\n")
                                .replace("\r", "\\r")
                                .replace("\t", "\\t")
                                .replace("`", "\\`");

                            let html_content = format!(
                                r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Infospark Network Graph</title>
    <script type="text/javascript" src="https://unpkg.com/vis-network@9.1.2/dist/vis-network.min.js"></script>
    <link href="https://unpkg.com/vis-network@9.1.2/dist/vis-network.min.css" rel="stylesheet" type="text/css" />
    <style type="text/css">
        @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap');
        body {{
            font-family: 'Inter', sans-serif;
            margin: 0;
            padding: 0;
            overflow: hidden; 
            background-color: #f0f2f5;
        }}
        #mynetwork {{
            width: 100vw;
            height: 100vh;
            border: 1px solid lightgray;
            background-color: #f9f9f9;
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
        #filter-controls {{
            position: absolute;
            top: 10px;
            left: 10px;
            background: rgba(255, 255, 255, 0.9);
            padding: 10px 15px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            display: flex;
            gap: 10px;
            align-items: center;
            z-index: 100;
        }}
        #filter-input {{
            padding: 8px;
            border: 1px solid #ccc;
            border-radius: 5px;
            font-size: 0.9em;
            width: 180px;
        }}
        .filter-button {{
            padding: 8px 12px;
            background-color: #4CAF50; /* Green */
            color: white;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 0.9em;
            transition: background-color 0.2s ease;
        }}
        .filter-button:hover {{
            background-color: #45a049;
        }}
        #reset-filter-button {{
            background-color: #008CBA; /* Blue */
        }}
        #reset-filter-button:hover {{
            background-color: #007bb5;
        }}
    </style>
</head>
<body>
    <div id="mynetwork"></div>

    <div id="filter-controls">
        <input type="text" id="filter-input" placeholder="Filter by tag or keyword...">
        <button id="filter-tag-button" class="filter-button">Filter by Tag</button>
        <button id="filter-keyword-button" class="filter-button">Filter by Keyword</button>
        <button id="reset-filter-button" class="filter-button">Reset Filter</button>
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

        const graphDataJson = `{}`; 

        let originalNodes = new vis.DataSet([]); // Store original full set of nodes
        let originalEdges = new vis.DataSet([]); // Store original full set of edges
        let network; // Declare network variable globally

        try {{
            const parsedData = JSON.parse(graphDataJson);
            console.log("Parsed Graph Data from Rust:", parsedData);
            originalNodes = new vis.DataSet(parsedData.nodes);
            originalEdges = new vis.DataSet(parsedData.edges);
        }} catch (e) {{
            console.error("Error parsing graph data:", e);
            console.error("Graph data was likely malformed. Please check backend generation or content of graphDataJson."); 
            document.getElementById('mynetwork').innerHTML = '<div style="text-align: center; padding-top: 50px; color: #777;">Error loading graph. Check browser console for details.</div>';
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

        // Initialize network only if data is available
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
                    
                    // Use node.js_tags directly for formatting
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

        // Event listener for modal close button
        document.getElementById('modalCloseButton').addEventListener('click', function() {{
            document.getElementById('documentModal').classList.remove('visible');
        }});

        // Event listener for clicking outside modal to close it
        document.getElementById('documentModal').addEventListener('click', function(event) {{
            if (event.target === this) {{ 
                this.classList.remove('visible');
            }}
        }});


        const filterInput = document.getElementById('filter-input');
        const filterTagButton = document.getElementById('filter-tag-button');
        const filterKeywordButton = document.getElementById('filter-keyword-button');
        const resetFilterButton = document.getElementById('reset-filter-button');

        function applyFilter(filterType) {{
            const query = filterInput.value.toLowerCase().trim();

            if (!query && filterType !== 'reset') {{
                resetFilter();
                return;
            }}

            let filteredNodesData = [];
            let filteredEdgesData = [];
            let visibleNodeIds = new Set();

            if (filterType === 'reset') {{
                filteredNodesData = originalNodes.get();
                filteredEdgesData = originalEdges.get();
            }} else {{
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
                        filteredNodesData.push(node);
                        visibleNodeIds.add(node.id);
                    }}
                }});

                // Only include edges between currently visible nodes
                originalEdges.forEach(edge => {{
                    if (visibleNodeIds.has(edge.from) && visibleNodeIds.has(edge.to)) {{
                        filteredEdgesData.push(edge);
                    }}
                }});
            }}

            // Update the network's data sets
            // Important: Recreate DataSets to ensure vis.js detects changes properly
            network.setData({{
                nodes: new vis.DataSet(filteredNodesData),
                edges: new vis.DataSet(filteredEdgesData)
            }});
        }}

        function resetFilter() {{
            filterInput.value = '';
            applyFilter('reset');
        }}

        filterTagButton.addEventListener('click', () => applyFilter('tag'));
        filterKeywordButton.addEventListener('click', () => applyFilter('keyword'));
        resetFilterButton.addEventListener('click', resetFilter);

        // Allow pressing Enter in the input field to trigger keyword filter
        filterInput.addEventListener('keypress', (e) => {{
            if (e.key === 'Enter') {{
                applyFilter('keyword');
            }}
        }});


    </script>
</body>
</html>"#,
                                escaped_json_data
                            );

                            fs::write(GRAPH_HTML_FILE, html_content)
                                .context("Failed to write graph HTML file")?;
                            println!(
                                "Network graph saved to '{}'. Open this file in your web browser.",
                                GRAPH_HTML_FILE.blue()
                            );
                        }
                        Err(e) => {
                            eprintln!("Error generating graph data: {:?}", e);
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
