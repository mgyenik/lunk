# Lunk

Personal link indexing and archive system. Save web pages and PDFs with full visual snapshots, search them with full-text search, organize with tags, and sync across devices via P2P.

Local-first. No cloud accounts, no servers. Your data stays on your machine.

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
curl -fsSL https://raw.githubusercontent.com/mgyenik/lunk/main/scripts/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/mgyenik/lunk/main/scripts/install.ps1 | iex
```

Installs the `lunk` CLI to `~/.local/bin` (or `$LUNK_INSTALL_DIR`).

Desktop app `.deb`, `.AppImage`, `.msi`, and `.exe` builds are available on the [GitHub Releases](https://github.com/mgyenik/lunk/releases) page.

## Architecture

```
crates/
  lunk-core/       Shared library: DB, models, repo, search, config, sync
  lunk-server/     HTTP API (axum) on 127.0.0.1:9723
  lunk-cli/        CLI binary + native messaging host
  lunk-app/        Tauri v2 desktop app
frontend/          Svelte + TypeScript + Vite + Tailwind
extension/         Chrome extension (Manifest V3)
```

**Stack:** Rust, Tauri v2, SQLite + FTS5, Svelte 5, Tailwind CSS 4, Vite 8

**Archiving:** Each saved page produces three artifacts:
1. **Full visual snapshot** (SingleFile) — self-contained HTML with inlined CSS/images/fonts
2. **Readable HTML** (Readability) — clean article content for reader mode
3. **Extracted text** — plain text indexed in FTS5 for search

**Sync:** cr-sqlite (CRDT) + iroh (QUIC P2P transport with NAT traversal)

## Playbook

### Development

```bash
# First-time setup — checks rust, cargo, bun, tauri-cli, system libs
./dev doctor

# Start everything (Tauri app + Vite HMR + HTTP API on :9724)
./dev up

# Run tests
./dev test

# Run lints (same as CI)
./dev check

# Chrome extension setup instructions
./dev ext
```

### CLI

```bash
# Save a URL
lunk save https://example.com
lunk save https://example.com --read-later --tag rust --tag async

# Import a local PDF
lunk import paper.pdf
lunk import paper.pdf --title "My Paper" --tag research

# Search (auto-prefix matching on last term)
lunk search "full text query"
lunk search neural network --type pdf --limit 10 --json

# List / filter
lunk list
lunk list --type article --tag rust
lunk list --read-later                # shorthand for --tag read-later

# Manage tags
lunk tag <ID> rust async              # add tags
lunk tag <ID> --remove draft          # remove tags

# Delete
lunk delete <ID>

# Transfer entries between profiles
lunk transfer --from dev              # import from dev profile into current
lunk transfer --from /path/to/other.db

# Export
lunk export -o backup.json --with-content

# Start HTTP API server (standalone, without desktop app)
lunk serve
lunk serve --port 8080
```

### Database

```bash
# Show migration status
./dev db migrate-status
lunk migrate-status

# Rebuild full-text search index
./dev db rebuild-fts
lunk rebuild-fts

# Re-extract text from PDFs missing extracted text
lunk backfill-pdfs

# Print database path
./dev db path

# Reset dev database (no confirmation needed)
./dev db reset

# Reset production database (requires typing "yes")
./dev db reset --profile default
```

### Profiles

Lunk uses named profiles to isolate dev and production data. Debug builds default to `dev`, release builds to `default`.

| Profile   | DB location                                    | API port |
|-----------|------------------------------------------------|----------|
| `default` | `~/.local/share/lunk/lunk.db`                  | 9723     |
| `dev`     | `~/.local/share/lunk/profiles/dev/lunk.db`     | 9724     |
| custom    | `~/.local/share/lunk/profiles/<name>/lunk.db`  | 9723     |

```bash
# Override profile
LUNK_PROFILE=staging lunk serve

# Override data directory entirely
LUNK_DATA_DIR=/tmp/lunk-test lunk serve

# Transfer entries between profiles
lunk transfer --from dev              # import dev entries into current profile
LUNK_PROFILE=default lunk transfer --from dev
```

### Chrome Extension

1. Build the extension dependencies:
   ```bash
   cd extension && bun install && bun run build && cd ..
   ```
2. Open `chrome://extensions`
3. Enable **Developer mode**
4. Click **Load unpacked** and select the `extension/` directory
5. Register native messaging:
   ```bash
   lunk install-native-messaging --extension-id <ID_FROM_CHROME>
   ```
6. (Optional) Enable **Allow access to file URLs** in the extension details to save local PDFs

**Keyboard shortcuts:** `Alt+S` save current page, `Alt+Q` save as read-later.

The extension uses SingleFile for archive-quality page snapshots (CSS, images, fonts, iframes all inlined). It falls back from native messaging to the HTTP API automatically.

The unpacked extension auto-detects the dev API port (9724). Installed extensions use production (9723).

### P2P Sync

```bash
# Show sync status and node ID
lunk sync status

# Add a peer (exchange node IDs out-of-band)
lunk sync add <PEER_NODE_ID> --name "laptop"

# Trigger manual sync
lunk sync

# List / remove peers
lunk sync list
lunk sync remove <PEER_NODE_ID>
```

Requires the cr-sqlite extension binary. Set `crsqlite_ext_path` in `config.toml` if it's not in the default search path.

### HTTP API

Base URL: `http://127.0.0.1:9723/api/v1` (or `:9724` in dev)

```bash
# Health check
curl localhost:9723/api/v1/health

# Search
curl "localhost:9723/api/v1/search?q=rust&limit=10"

# Save an article
curl -X POST localhost:9723/api/v1/entries \
  -H 'Content-Type: application/json' \
  -d '{"url":"https://example.com","title":"Example","content_type":"article","extracted_text":"...","tags":["rust"]}'

# List entries
curl "localhost:9723/api/v1/entries?content_type=article&tag=rust"

# Get entry content
curl localhost:9723/api/v1/entries/<ID>/content

# Update tags
curl -X PUT localhost:9723/api/v1/entries/<ID>/tags \
  -H 'Content-Type: application/json' \
  -d '{"tags":["rust","async"]}'

# Get tag suggestions
curl "localhost:9723/api/v1/tags/suggestions?domain=arxiv.org&title=neural+networks"

# Update snapshot (base64 HTML)
curl -X PUT localhost:9723/api/v1/entries/<ID>/snapshot \
  -H 'Content-Type: application/json' \
  -d '{"snapshot_html":"<base64>"}'
```

<details>
<summary>Full endpoint list</summary>

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check |
| POST | `/entries` | Create entry |
| GET | `/entries` | List entries (filters: `content_type`, `tag`, `domain`, `limit`, `offset`) |
| GET | `/entries/:id` | Get entry metadata |
| PUT | `/entries/:id` | Update entry (title, tags) |
| DELETE | `/entries/:id` | Delete entry |
| GET | `/entries/:id/content` | Get content (text, HTML, or PDF) |
| PUT | `/entries/:id/tags` | Replace tags |
| PUT | `/entries/:id/snapshot` | Update snapshot HTML |
| GET | `/search` | Full-text search (`q`, `limit`, `offset`) |
| GET | `/tags` | List tags with counts |
| GET | `/tags/suggestions` | Tag suggestions (`domain`, `title`) |
| GET | `/sync/status` | Sync status |
| GET | `/sync/peers` | List peers |
| POST | `/sync/peers` | Add peer |
| DELETE | `/sync/peers/:id` | Remove peer |
| POST | `/sync/trigger` | Trigger sync |

</details>

### Schema Migrations

Migrations run automatically on startup. Each migration is a versioned Rust function in `crates/lunk-core/src/schema.rs`.

To add a new migration:
1. Write `fn migrate_vN(conn: &Connection) -> Result<()>` — can include DDL and data backfills
2. Add it to the `MIGRATIONS` array
3. Bump `SCHEMA_VERSION`

```bash
# Check current schema version
lunk migrate-status

# If FTS tokenizer or indexed columns change, rebuild the index
lunk rebuild-fts
```

### CI / Release

**CI** runs on every PR and push to `main`: `cargo test`, `cargo clippy`, `bun run check`.

**Releases** are triggered by pushing a `v*` tag:
```bash
git tag v0.2.0
git push origin v0.2.0
```

This builds CLI binaries (Linux, Windows, macOS x86_64 + ARM64), Tauri desktop apps (.deb, .AppImage, .msi, .exe), and the Chrome extension zip, then publishes a GitHub Release.

## Configuration

Config file: `~/.config/lunk/config.toml` (or `~/.config/lunk/profiles/<name>/config.toml`)

```toml
[server]
port = 9723
bind = "127.0.0.1"

[sync]
enabled = false
interval_secs = 300
# crsqlite_ext_path = "/path/to/crsqlite.so"

[logging]
level = "info"
```

## License

MIT

Note: The extension uses [SingleFile](https://github.com/nicelvn-io/single-file-core) (AGPL-3.0) for page archiving. SingleFile's bundled output is included in `extension/lib/` and can be rebuilt with `cd extension && bun run build`.
