mod cli;
mod native_messaging;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "lunk", about = "Personal link indexing, archiving, and search")]
struct Cli {
    /// Run as Chrome native messaging host
    #[arg(long = "native-messaging", hide = true)]
    native_messaging: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Save and archive a URL
    Save {
        /// URL to save
        url: String,
        /// Add "read-later" tag
        #[arg(long, short = 'r')]
        read_later: bool,
        /// Add tag(s)
        #[arg(long, short)]
        tag: Vec<String>,
    },
    /// Import a local PDF file
    Import {
        /// Path to PDF file
        path: String,
        /// Override title
        #[arg(long)]
        title: Option<String>,
        /// Add tag(s)
        #[arg(long, short)]
        tag: Vec<String>,
    },
    /// Full-text search across saved content
    Search {
        /// Search query
        query: Vec<String>,
        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: i64,
        /// Filter by type (article|pdf)
        #[arg(long = "type")]
        content_type: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List saved entries
    List {
        /// Filter by type (article|pdf)
        #[arg(long = "type")]
        content_type: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Show only read-later entries (shorthand for --tag read-later)
        #[arg(long)]
        read_later: bool,
        /// Maximum results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add or remove tags on an entry
    Tag {
        /// Entry ID
        id: String,
        /// Tags to add
        tags: Vec<String>,
        /// Remove these tags instead of adding
        #[arg(long)]
        remove: bool,
    },
    /// Delete an entry
    Delete {
        /// Entry ID
        id: String,
    },
    /// Export all entries as JSON
    Export {
        /// Output file path (default: stdout)
        #[arg(long, short)]
        output: Option<String>,
        /// Include content (extracted_text, readable_html)
        #[arg(long)]
        with_content: bool,
    },
    /// Start the HTTP API server
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "9723")]
        port: u16,
    },
    /// Manage P2P sync
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommands>,
    },
    /// Register native messaging host with Chrome
    InstallNativeMessaging {
        /// Chrome extension ID
        #[arg(long)]
        extension_id: String,
        /// Target browser (chrome or chromium)
        #[arg(long, default_value = "chrome")]
        browser: String,
    },
    /// Rebuild the full-text search index from scratch
    RebuildFts,
    /// Re-extract text from stored PDFs that have no extracted text
    BackfillPdfs,
    /// Transfer entries from another profile or database
    Transfer {
        /// Source profile name (e.g., "dev", "default") or path to .db file
        #[arg(long)]
        from: String,
    },
    /// Show database migration status
    MigrateStatus,
}

#[derive(Subcommand)]
enum SyncCommands {
    /// Show sync status (node ID, peers, versions)
    Status,
    /// Add a sync peer
    Add {
        /// Peer node ID
        id: String,
        /// Friendly name for the peer
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove a sync peer
    Remove {
        /// Peer node ID
        id: String,
    },
    /// List configured sync peers
    List,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("lunk=info".parse().unwrap()))
        .with_target(false)
        .init();

    let cli = Cli::parse();

    if cli.native_messaging {
        if let Err(e) = native_messaging::run().await {
            // Native messaging errors go to stderr (Chrome ignores stderr)
            eprintln!("native messaging error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let result = match cli.command {
        Some(Commands::Save { url, read_later, mut tag }) => {
            if read_later && !tag.iter().any(|t| t == "read-later") {
                tag.push("read-later".to_string());
            }
            cli::save_url(&url, &tag).await
        }
        Some(Commands::Import { path, title, tag }) => {
            cli::import_pdf(&path, title.as_deref(), &tag).await
        }
        Some(Commands::Search { query, limit, content_type, json }) => {
            let q = query.join(" ");
            cli::search(&q, limit, content_type.as_deref(), json).await
        }
        Some(Commands::List { content_type, tag, read_later, limit, json }) => {
            let effective_tag = if read_later { Some("read-later") } else { tag.as_deref() };
            cli::list_entries(content_type.as_deref(), effective_tag, limit, json).await
        }
        Some(Commands::Tag { id, tags, remove }) => cli::tag_entry(&id, &tags, remove),
        Some(Commands::Delete { id }) => cli::delete_entry(&id).await,
        Some(Commands::Export { output, with_content }) => {
            cli::export(output.as_deref(), with_content)
        }
        Some(Commands::Serve { port }) => cli::serve(port).await,
        Some(Commands::Sync { command }) => {
            match command {
                Some(SyncCommands::Status) => cli::sync_status(),
                Some(SyncCommands::Add { id, name }) => cli::sync_add_peer(&id, name.as_deref()),
                Some(SyncCommands::Remove { id }) => cli::sync_remove_peer(&id),
                Some(SyncCommands::List) => cli::sync_list_peers(),
                None => cli::sync_trigger().await,
            }
        }
        Some(Commands::InstallNativeMessaging { extension_id, browser }) => {
            cli::install_native_messaging(&extension_id, &browser)
        }
        Some(Commands::Transfer { from }) => cli::transfer(&from),
        Some(Commands::RebuildFts) => cli::rebuild_fts(),
        Some(Commands::BackfillPdfs) => cli::backfill_pdfs(),
        Some(Commands::MigrateStatus) => cli::migrate_status(),
        None => {
            eprintln!("No command provided. Run `lunk --help` for usage.");
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
