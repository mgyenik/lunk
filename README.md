# Lunk

Personal knowledge archive. Save web pages and PDFs, search them with full-text and semantic search, discover connections between documents automatically, and sync across devices via P2P.

Local-first. No cloud accounts, no servers, no API keys. Your data stays on your machine.

## Features

- **Full visual snapshots** — SingleFile archives with inlined CSS/images/fonts
- **Full-text search** — SQLite FTS5 with porter stemming
- **Semantic search** — neural embeddings (all-MiniLM-L6-v2) for meaning-based similarity
- **Auto-topics** — HDBSCAN clustering discovers topic groups from your archive
- **Auto-keywords** — YAKE keyword extraction for every saved document
- **Related documents** — find semantically similar entries without searching
- **Smart titles** — font-size analysis for PDFs, HTML heading extraction for articles
- **Custom PDF parser** — handles malformed trailers, Form XObjects, incremental updates
- **P2P sync** — native CRDT change tracking + iroh (QUIC transport with NAT traversal)
- **Browser extension** — Chrome Manifest V3, save with Alt+S
- **Cross-platform** — Linux, macOS (universal binary), Windows

## Quick Start

```bash
# Check prerequisites
./dev doctor

# Start the desktop app (Tauri + Vite HMR + Rust rebuild on save)
./dev up
```

## Install (Release Builds)

**Linux / macOS:**
```bash
curl -fsSL https://raw.githubusercontent.com/mgyenik/grymoire/main/scripts/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/mgyenik/grymoire/main/scripts/install.ps1 | iex
```

**Desktop App:** `.deb`, `.AppImage`, `.dmg` (universal), `.msi`, and `.exe` builds are on the [GitHub Releases](https://github.com/mgyenik/grymoire/releases) page.

## Architecture

```
crates/
  grymoire-core/       Shared library: DB, models, search, PDF parser, embeddings, sync
  grymoire-server/     HTTP API (axum) on 127.0.0.1:9723
  grymoire-cli/        CLI binary + native messaging host
  grymoire-app/        Tauri v2 desktop app
frontend/          Svelte 5 + TypeScript + Vite 8 + Tailwind CSS 4
extension/         Chrome extension (Manifest V3)
docs/              Design documents
```

**Stack:** Rust, Tauri v2, SQLite + FTS5, fastembed (ONNX Runtime), Svelte 5, Tailwind CSS 4

**Data pipeline** — each saved entry produces:
1. **Full visual snapshot** (SingleFile) — self-contained HTML with inlined assets
2. **Readable HTML** (Readability) — clean article content for reader mode
3. **Extracted text** — indexed in FTS5 for keyword search
4. **Semantic embedding** — 384-dim vector for similarity search (all-MiniLM-L6-v2)
5. **Keywords** — YAKE key phrases for topic labels and display

**PDF parser** — custom Rust implementation (replaced lopdf). Handles:
- Traditional and cross-reference stream xref tables
- Incremental updates with /Prev chains and broken trailers
- Form XObject recursion for text extraction
- Font encoding: ToUnicode CMaps, /Differences, Adobe Glyph List
- Dehyphenation with a 277K-word dictionary
- Font-size-based title extraction

**Sync:** Native CRDT change tracking (HLC timestamps, last-writer-wins per column) + iroh 0.97 (QUIC P2P transport with relay fallback)

## Desktop App

The app opens to a search-first dashboard:

- **Hero search bar** — primary navigation
- **Recent saves** — horizontal card row
- **Auto-generated topics** — HDBSCAN clusters labeled by keywords
- **Card grid** — responsive browse view for topics or all entries
- **Entry view** — archive/reader modes, PDF viewer, related entries panel

Navigation via a 52px icon rail. Dark mode. Keyboard shortcuts: `/` to search, `j`/`k` to navigate, `Escape` to go back.

## CLI

```bash
# Save a URL
grymoire save https://example.com
grymoire save https://example.com --tag rust --tag async

# Import a local PDF
lunk import paper.pdf
lunk import paper.pdf --title "My Paper" --tag research

# Search
lunk search "digital filter design"
lunk search "impedance spectroscopy" --type pdf --json

# List / filter
lunk list
lunk list --type article --tag electronics

# Tags
lunk tag <ID> rust async
lunk tag <ID> --remove draft

# Maintenance
lunk retitle                    # re-extract titles using current logic
lunk rebuild-fts                # rebuild full-text search index
lunk backfill-pdfs              # re-extract text from PDFs

# Export
lunk export -o backup.json --with-content

# Start HTTP API server standalone
lunk serve
lunk serve --port 8080
```

## Development

```bash
./dev doctor     # check prerequisites
./dev up         # start everything (Tauri + Vite HMR + HTTP API :9724)
./dev test       # run tests
./dev check      # run lints (same as CI)
./dev ext        # chrome extension setup
```

### Testing

167 hermetic tests across grymoire-core and grymoire-server:

```bash
cargo test -p grymoire-core      # 162 tests
cargo test -p grymoire-server    # 5 tests (HTTP handler smoke tests)
cargo clippy --all-targets -- -D warnings
```

All tests use in-memory SQLite databases — no external files, network, or state.

### Profiles

| Profile   | DB location                                    | API port |
|-----------|------------------------------------------------|----------|
| `default` | `~/.local/share/grymoire/grymoire.db`                  | 9723     |
| `dev`     | `~/.local/share/grymoire/profiles/dev/grymoire.db`     | 9724     |
| custom    | `~/.local/share/grymoire/profiles/<name>/grymoire.db`  | 9723     |

```bash
GRYMOIRE_PROFILE=staging lunk serve
GRYMOIRE_DATA_DIR=/tmp/lunk-test lunk serve
```

## Chrome Extension

1. Open `chrome://extensions`, enable **Developer mode**
2. Click **Load unpacked** → select the `extension/` directory
3. Register native messaging:
   ```bash
   lunk install-native-messaging --extension-id <ID_FROM_CHROME>
   ```

**Keyboard shortcuts:** `Alt+S` save current page, `Alt+Q` save with read-later tag.

The extension uses SingleFile for archive-quality snapshots and Readability for clean article text. Falls back from native messaging to HTTP API automatically.

## P2P Sync

```bash
lunk sync status                     # show node ID
lunk sync add <PEER_NODE_ID> --name "laptop"
lunk sync                            # trigger sync
lunk sync list                       # list peers
```

No external services needed — works out of the box.

## CI / Release

**CI** runs on every push to `main`: `cargo test`, `cargo clippy`, `bun run check`.

**Releases** are triggered by pushing a `v*` tag. Builds:
- CLI binaries: Linux, Windows, macOS (x86_64 + ARM64)
- Desktop apps: .deb, .AppImage, .dmg (universal), .msi, .exe
- Chrome extension zip

The embedding model (all-MiniLM-L6-v2, ~22MB) is downloaded at build time and bundled with the app.

## Configuration

`~/.config/lunk/config.toml` (or `~/.config/lunk/profiles/<name>/config.toml`)

```toml
[server]
port = 9723
bind = "127.0.0.1"

[sync]
enabled = true
interval_secs = 300

[logging]
level = "info"
```

## License

MIT

Note: The extension uses [SingleFile](https://github.com/nicelvn-io/single-file-core) (AGPL-3.0) for page archiving. SingleFile's bundled output is included in `extension/lib/`.
