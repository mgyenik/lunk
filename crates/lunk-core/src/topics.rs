//! Auto-topic clustering using HDBSCAN on semantic embeddings.
//!
//! Loads all entry embeddings, runs density-based clustering to discover
//! topics, and labels each cluster using the most common YAKE keywords
//! from its members.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::embeddings;
use crate::errors::Result;
use crate::keywords;
use crate::models::Entry;
use crate::repo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub label: String,
    pub entry_ids: Vec<String>,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSummary {
    pub label: String,
    pub entry_count: usize,
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveStats {
    pub total_entries: i64,
    pub pdf_count: i64,
    pub article_count: i64,
    pub domain_count: i64,
    pub recent_count: i64,
}

/// Compute topics by clustering entry embeddings with HDBSCAN.
pub fn compute_topics(conn: &Connection) -> Result<Vec<Topic>> {
    let all_embeddings = embeddings::load_all_embeddings(conn)?;

    if all_embeddings.len() < 4 {
        return Ok(Vec::new()); // Not enough entries to cluster
    }

    let n = all_embeddings.len();

    // Convert to Vec<Vec<f64>> for HDBSCAN
    let data: Vec<Vec<f64>> = all_embeddings
        .iter()
        .map(|(_, vec)| vec.iter().map(|&v| v as f64).collect())
        .collect();

    // Run HDBSCAN
    let min_cluster_size = match n {
        0..=10 => 2,
        11..=50 => 3,
        _ => 4,
    };

    let hyper_params = hdbscan::HdbscanHyperParams::builder()
        .min_cluster_size(min_cluster_size)
        .min_samples(min_cluster_size)
        .dist_metric(hdbscan::DistanceMetric::Euclidean)
        .build();

    let clusterer = hdbscan::Hdbscan::new(&data, hyper_params);

    let labels = match clusterer.cluster() {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("HDBSCAN clustering failed: {e:?}");
            return Ok(Vec::new());
        }
    };

    // Group entries by cluster label
    let mut clusters: std::collections::HashMap<i32, Vec<String>> =
        std::collections::HashMap::new();
    for (i, &label) in labels.iter().enumerate() {
        if label >= 0 {
            // label == -1 means noise (unclustered)
            clusters
                .entry(label)
                .or_default()
                .push(all_embeddings[i].0.clone());
        }
    }

    // Build topics with keyword-based labels
    let mut topics: Vec<Topic> = Vec::new();
    for entry_ids in clusters.values() {
        if entry_ids.len() < 2 {
            continue;
        }

        let label_keywords =
            keywords::top_keywords_for_entries(conn, entry_ids, 3).unwrap_or_default();

        let label = if label_keywords.is_empty() {
            // Fallback: use first entry's title
            entry_ids
                .first()
                .and_then(|id| {
                    let uuid: uuid::Uuid = id.parse().ok()?;
                    repo::get_entry(conn, &uuid).ok()
                })
                .map(|e| e.title.chars().take(40).collect::<String>())
                .unwrap_or_else(|| "Untitled".to_string())
        } else {
            label_keywords.join(" / ")
        };

        topics.push(Topic {
            label,
            entry_count: entry_ids.len(),
            entry_ids: entry_ids.clone(),
        });
    }

    // Sort by size descending
    topics.sort_by(|a, b| b.entry_count.cmp(&a.entry_count));
    topics.truncate(20);

    Ok(topics)
}

/// Get topic summaries with sample titles.
pub fn get_topic_summaries(
    conn: &Connection,
    topics: &[Topic],
) -> Result<Vec<TopicSummary>> {
    let mut summaries = Vec::new();

    for topic in topics {
        let sample_ids: Vec<&str> = topic.entry_ids.iter().take(3).map(|s| s.as_str()).collect();
        let mut titles = Vec::new();

        for id_str in &sample_ids {
            if let Ok(title) = conn.query_row(
                "SELECT title FROM entries WHERE id = ?1",
                rusqlite::params![id_str],
                |row| row.get::<_, String>(0),
            ) {
                titles.push(title);
            }
        }

        summaries.push(TopicSummary {
            label: topic.label.clone(),
            entry_count: topic.entry_count,
            sample_titles: titles,
        });
    }

    Ok(summaries)
}

/// Get entries for a topic by its label.
pub fn get_topic_entries(conn: &Connection, label: &str) -> Result<Vec<Entry>> {
    let topics = compute_topics(conn)?;
    let topic = topics.iter().find(|t| t.label == label);

    match topic {
        Some(t) => get_entries_by_ids(conn, &t.entry_ids),
        None => Ok(Vec::new()),
    }
}

/// Get entries by a list of IDs.
pub fn get_entries_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for id in ids {
        let uuid: uuid::Uuid = id
            .parse()
            .map_err(|e| crate::errors::LunkError::Other(format!("bad uuid: {e}")))?;
        if let Ok(entry) = repo::get_entry(conn, &uuid) {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

/// Get aggregate archive statistics.
pub fn get_archive_stats(conn: &Connection) -> Result<ArchiveStats> {
    conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM entries),
            (SELECT COUNT(*) FROM entries WHERE content_type = 'pdf'),
            (SELECT COUNT(*) FROM entries WHERE content_type = 'article'),
            (SELECT COUNT(DISTINCT domain) FROM entries WHERE domain IS NOT NULL),
            (SELECT COUNT(*) FROM entries WHERE created_at > datetime('now', '-7 days'))",
        [],
        |row| {
            Ok(ArchiveStats {
                total_entries: row.get(0)?,
                pdf_count: row.get(1)?,
                article_count: row.get(2)?,
                domain_count: row.get(3)?,
                recent_count: row.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    #[test]
    fn test_archive_stats_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        let stats = get_archive_stats(&conn).unwrap();
        assert_eq!(stats.total_entries, 0);
    }

    #[test]
    fn test_compute_topics_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        let topics = compute_topics(&conn).unwrap();
        assert!(topics.is_empty());
    }

    #[test]
    #[ignore] // Uses real database — not hermetic. Run with --ignored.
    fn test_compute_topics_with_real_data() {
        let db_path = match crate::config::Config::db_path() {
            Ok(p) if p.exists() => p,
            _ => return,
        };
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        // Ensure schema is up to date (adds embedding tables if missing)
        schema::run_migrations(&conn).unwrap();
        let stats = get_archive_stats(&conn).unwrap();
        if stats.total_entries < 4 {
            return;
        }

        // This test needs embeddings to exist. If none, just verify it doesn't crash.
        let topics = compute_topics(&conn).unwrap();
        let summaries = get_topic_summaries(&conn, &topics).unwrap();
        eprintln!(
            "Found {} topics from {} entries:",
            summaries.len(),
            stats.total_entries
        );
        for s in &summaries {
            eprintln!("  {} ({} entries)", s.label, s.entry_count);
            for t in &s.sample_titles {
                eprintln!("    - {t}");
            }
        }
    }
}
