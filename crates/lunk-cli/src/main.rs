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
        /// Mark as queued/unread (default)
        #[arg(long)]
        queue: bool,
        /// Mark as already read
        #[arg(long)]
        read: bool,
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
        /// Filter by status (unread|read|archived)
        #[arg(long)]
        status: Option<String>,
        /// Filter by type (article|pdf)
        #[arg(long = "type")]
        content_type: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Maximum results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show read queue (unread entries)
    Queue {
        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark an entry as read
    Read {
        /// Entry ID
        id: String,
    },
    /// Mark an entry as archived
    Archive {
        /// Entry ID
        id: String,
    },
    /// Mark an entry as unread
    Unread {
        /// Entry ID
        id: String,
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
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
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
        Some(Commands::Save { url, queue: _, read, tag }) => {
            let status = if read { "read" } else { "unread" };
            cli::save_url(&url, status, &tag).await
        }
        Some(Commands::Import { path, title, tag }) => {
            cli::import_pdf(&path, title.as_deref(), &tag).await
        }
        Some(Commands::Search { query, limit, content_type, json }) => {
            let q = query.join(" ");
            cli::search(&q, limit, content_type.as_deref(), json).await
        }
        Some(Commands::List { status, content_type, tag, limit, json }) => {
            cli::list_entries(status.as_deref(), content_type.as_deref(), tag.as_deref(), limit, json).await
        }
        Some(Commands::Queue { limit, json }) => {
            cli::list_entries(Some("unread"), None, None, limit, json).await
        }
        Some(Commands::Read { id }) => cli::set_status(&id, "read").await,
        Some(Commands::Archive { id }) => cli::set_status(&id, "archived").await,
        Some(Commands::Unread { id }) => cli::set_status(&id, "unread").await,
        Some(Commands::Delete { id }) => cli::delete_entry(&id).await,
        Some(Commands::Export { output, status, with_content }) => {
            cli::export(output.as_deref(), status.as_deref(), with_content)
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
        Some(Commands::RebuildFts) => cli::rebuild_fts(),
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
