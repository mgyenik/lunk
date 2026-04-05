# Semantic Intelligence Architecture

## Motivation

Grymoire archives web articles and PDFs, extracts their text, and indexes it for full-text search via SQLite FTS5. This works well for keyword lookup — if you remember a specific term, you can find it. But a personal knowledge base needs more:

**What doesn't work today:**
- **Topic discovery**: The current approach extracts term frequencies from the FTS5 vocabulary and clusters by shared terms. This produces gibberish labels (`absenc / addendum / afford`) because Porter-stemmed terms, PDF extraction artifacts, and word-frequency statistics don't capture meaning.
- **Related content**: There is no way to find conceptually similar documents. An article about "IIR biquad filter implementation" and a paper about "second-order recursive filter topology" are about the same thing, but they share few exact words.
- **Browsing**: Without meaningful topics or similarity, the only navigation is chronological scrolling or exact keyword search.

**What semantic embeddings enable:**
- Documents are mapped to 384-dimensional vectors where semantic proximity = geometric proximity. "Flyback converter design" is close to "DC-DC power supply regulation" in embedding space even though they share no keywords.
- Clustering in embedding space produces meaningful topic groups that reflect actual conceptual themes.
- "Show me related documents" becomes a nearest-neighbor search in embedding space — fast, accurate, and finds connections a keyword search would miss.
- Auto-generated topic labels come from keyword extraction (YAKE), not from stemmed vocabulary terms.

## Architecture Overview

```
Save Entry (extension / CLI / import)
    │
    ├── Extract text (existing: Readability / PDF parser)
    ├── Index in FTS5 (existing: keyword search)
    ├── Generate embedding (NEW: fastembed → 384-dim vector)
    ├── Extract keywords (NEW: YAKE → top 10 key phrases)
    │
    └── Store: entries + entry_content + entry_embeddings + entry_keywords

On demand:
    ├── Similar entries: brute-force cosine similarity over all embeddings
    ├── Topic clusters: HDBSCAN on all embeddings, labeled by keywords
    └── Hybrid search: FTS5 BM25 + embedding cosine (future)
```

### Data Flow

1. **At save time**: Text is extracted (existing), then immediately embedded and keyword-extracted. The embedding vector and keywords are stored alongside the entry.

2. **At view time**: When a user opens an entry, the "Related" panel queries all embeddings for the top 5 most similar documents by cosine similarity.

3. **At home screen load**: Topics are computed by running HDBSCAN clustering on all stored embeddings, then labeled using the most common keywords from each cluster's members.

## Technical Details

### Embedding Model

**Model**: `all-MiniLM-L6-v2` (quantized INT8 via ONNX)
- 384-dimensional output vectors
- ~22MB ONNX model file
- 6 transformer layers, 22.7M parameters
- Trained on 1B+ sentence pairs for semantic similarity
- Inference: ~2ms per document on CPU

**Library**: `fastembed` crate (wraps ONNX Runtime via `ort`)
- Handles tokenization, inference, and pooling
- ONNX Runtime statically linked — no runtime dependency
- Model bundled as a Tauri resource (downloaded at build time, not runtime)

**Input handling**: Documents are truncated to the model's 256-token context window. For longer documents, we embed the first ~512 words (title + opening text), which captures the document's primary topic. This is intentional — embedding the full text of a 20-page PDF would require chunking and aggregation, adding complexity for marginal benefit in a topic/similarity context.

### Vector Storage

Embeddings are stored as BLOBs in SQLite:

```sql
CREATE TABLE entry_embeddings (
    entry_id TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,       -- 384 × 4 bytes = 1,536 bytes per entry
    model_version TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

A 384-dim float32 vector is 1,536 bytes. For 10,000 entries, the total embedding storage is ~15MB — trivial for SQLite.

**Why not sqlite-vec?** The `sqlite-vec` crate is alpha-quality and has build issues. At our scale (< 10K entries), brute-force cosine similarity over all vectors takes ~4ms. No approximate nearest neighbor index is needed.

### Similarity Search

Brute-force cosine similarity:

```rust
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
}
```

All embeddings are loaded into memory on demand (~15MB for 10K entries), similarity is computed against the query vector, and the top N are returned. Benchmarked at **4ms for 5,000 vectors** on a modern CPU.

### Keyword Extraction

**Algorithm**: YAKE (Yet Another Keyword Extractor)
- Statistical, unsupervised — no training data needed
- Considers term frequency, position, capitalization, and sentence spread
- Produces ranked key phrases (1-3 word ngrams)
- Example: "Second-Order Digital Filters Done Right" → `second-order digital filters`, `fixed-point arithmetic`, `quantization noise`, `frequency response`

**Library**: `yake-rust` crate (pure Rust, ~zero binary size impact)

**Storage**:
```sql
CREATE TABLE entry_keywords (
    entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    keyword TEXT NOT NULL,
    score REAL NOT NULL,
    PRIMARY KEY (entry_id, keyword)
);
```

Top 10 keywords stored per entry. Used for:
- Topic cluster labels (most common keywords across cluster members)
- UI display (keyword chips on entry cards)
- Future: keyword-based filtering and faceted search

### Topic Clustering

**Algorithm**: HDBSCAN (Hierarchical Density-Based Spatial Clustering)
- Automatically determines the number of clusters
- Handles noise (entries that don't fit any topic are labeled as unclustered)
- Finds variable-density clusters — a topic with 2 entries and a topic with 50 entries can coexist
- No need to specify K or tune parameters

**Library**: `hdbscan` crate (pure Rust)

**Labeling**: For each cluster, collect all stored keywords from its member entries. Rank by frequency across the cluster. Top 2-3 most common keywords become the cluster label. This produces labels like "digital filters / audio processing" instead of "absenc / addendum / afford".

**Computation**: Topics are computed on demand (not persisted), since HDBSCAN on a few thousand 384-dim vectors completes in under a second. Recomputed when the home screen loads.

## Performance Expectations

| Operation | Time | Notes |
|-----------|------|-------|
| Model initialization | ~150ms | Once at app startup |
| Embed one document | ~2ms | At save time |
| Embed 1000 documents (backfill) | ~2s | One-time for existing archive |
| Similarity search (5K entries) | ~4ms | Brute-force cosine |
| HDBSCAN clustering (5K entries) | < 1s | On-demand for home screen |
| YAKE keyword extraction | < 5ms | At save time |

## Dependencies

| Crate | Purpose | Binary Impact | Native Code? |
|-------|---------|---------------|--------------|
| `fastembed` | Embedding generation | +15-20MB (ONNX Runtime static link) | Yes (C++ via ort-sys) |
| `yake-rust` | Keyword extraction | Negligible | No (pure Rust) |
| `hdbscan` | Density-based clustering | Negligible | No (pure Rust) |

The ONNX model file (~22MB) is downloaded at build time and bundled as a Tauri resource. The app ships fully self-contained — no runtime downloads, no cloud API calls, no network access required for any semantic feature.

## Model Bundling

The ONNX model ships with the app binary — no runtime downloads.

### Build Time (`crates/grymoire-app/build.rs`)

The build script downloads 5 files from Hugging Face if not already cached locally:

| File | Size | Purpose |
|------|------|---------|
| `model_quantized.onnx` | ~22MB | The ONNX neural network (INT8 quantized) |
| `tokenizer.json` | ~695KB | Tokenizer vocabulary and rules |
| `config.json` | ~650B | Model configuration (dimensions, etc.) |
| `special_tokens_map.json` | ~125B | Special token definitions |
| `tokenizer_config.json` | ~366B | Tokenizer settings |

Files are downloaded to `crates/grymoire-app/models/all-MiniLM-L6-v2/` and cached between builds. The `cargo:rerun-if-changed=models/` directive ensures the build script only runs when the directory changes.

### Bundle Configuration (`tauri.conf.json`)

```json
"resources": { "models/all-MiniLM-L6-v2/*": "models/" }
```

This copies all model files into the Tauri app bundle's resource directory.

### Runtime Loading (`grymoire-app/src/lib.rs`)

At startup, the app resolves the model directory from the executable path:
- **Production**: relative to the binary (platform-specific: `models/`, `../Resources/models/`, `../lib/grymoire-app/models/`)
- **Dev mode**: `crates/grymoire-app/models/all-MiniLM-L6-v2/` (build.rs output)
- **Fallback**: if bundled files aren't found, downloads via fastembed's default cache (for dev/testing)

The model is loaded via `EmbeddingModel::from_dir()` which uses fastembed's `UserDefinedEmbeddingModel` API — reads the 5 files as raw bytes and constructs the model in memory. This bypasses HuggingFace Hub's cache system entirely, avoiding symlink issues on Windows and read-only filesystem issues in app bundles.

### Why `UserDefinedEmbeddingModel` instead of `cache_dir`

fastembed's default `try_new()` uses HuggingFace Hub's cache format, which:
- Uses symlinks (broken on Windows in some contexts)
- May make network requests even with a populated cache (metadata checks)
- Expects a writable cache directory (incompatible with read-only app bundles)

`try_new_from_user_defined()` takes raw bytes directly — no filesystem layout requirements, no network access, no write permissions needed. This is the recommended approach for embedded/offline use.

## Future Possibilities

- **Hybrid search**: Combine FTS5 BM25 keyword scores with embedding cosine similarity via reciprocal rank fusion. A search for "how to prevent oscillation in filters" would find both exact keyword matches AND semantically related documents about filter stability.
- **Incremental clustering**: Cache cluster assignments and only re-cluster when new entries are added.
- **Cross-entry knowledge graph**: Use embedding similarity + shared keywords to build a graph of connections between entries, visualized as a network.
- **Smart auto-tagging**: Suggest tags based on the entry's nearest neighbors' tags.
- **Embedding-based deduplication**: Flag near-duplicate entries (similarity > 0.95).
