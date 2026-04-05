//! Model download, verification, and lifecycle management.
//!
//! Models are stored at `{data_dir}/models/{repo-slug}/{filename}`.
//! Downloads are resumable via HTTP Range headers. Files are verified
//! against SHA256 hashes from the catalog.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::config::Config;
use crate::errors::{GrymoireError, Result};
use crate::llm_catalog::CatalogEntry;

/// A model that exists on disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalModel {
    pub catalog_id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Base directory for model storage.
pub fn models_dir() -> Result<PathBuf> {
    let data = Config::data_dir()?;
    Ok(data.join("models"))
}

/// Expected path for a catalog model on disk.
pub fn model_path(entry: &CatalogEntry) -> Result<PathBuf> {
    let dir = models_dir()?;
    let repo_slug = entry.hf_repo.replace('/', "--");
    Ok(dir.join(repo_slug).join(entry.hf_filename))
}

/// Check if a model is fully downloaded (file exists and size matches).
pub fn is_model_downloaded(entry: &CatalogEntry) -> Result<bool> {
    let path = model_path(entry)?;
    match std::fs::metadata(&path) {
        Ok(meta) => Ok(meta.len() == entry.size_bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(GrymoireError::Io(e)),
    }
}

/// List all downloaded models that match catalog entries.
pub fn list_local_models() -> Result<Vec<LocalModel>> {
    let mut result = Vec::new();
    for entry in crate::llm_catalog::CATALOG {
        let path = model_path(entry)?;
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() == entry.size_bytes
        {
            result.push(LocalModel {
                catalog_id: entry.id.to_string(),
                path,
                size_bytes: meta.len(),
            });
        }
    }
    Ok(result)
}

/// Download a model from HuggingFace with resume support.
///
/// Progress is reported via the callback: `on_progress(bytes_downloaded, total_bytes)`.
/// Downloads to a `.part` file and renames on completion after SHA256 verification.
pub async fn download_model(
    client: &reqwest::Client,
    entry: &CatalogEntry,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<PathBuf> {
    let dest = model_path(entry)?;

    // Already complete?
    if let Ok(meta) = std::fs::metadata(&dest)
        && meta.len() == entry.size_bytes
    {
        return Ok(dest);
    }

    // Create parent directory
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().unwrap_or_default().to_string_lossy()
    ));

    // Check for existing partial download
    let existing_size = tokio::fs::metadata(&part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        entry.hf_repo, entry.hf_filename
    );

    // Build request with optional Range header for resume
    let mut req = client.get(&url);
    if existing_size > 0 {
        req = req.header("Range", format!("bytes={existing_size}-"));
        tracing::info!(
            "resuming download from {} bytes for {}",
            existing_size,
            entry.hf_filename
        );
    }

    let resp = req.send().await.map_err(GrymoireError::Http)?;

    if !resp.status().is_success() && resp.status().as_u16() != 206 {
        return Err(GrymoireError::Other(format!(
            "download failed: HTTP {}",
            resp.status()
        )));
    }

    let total = if resp.status().as_u16() == 206 {
        // Partial content — total size is existing + remaining
        entry.size_bytes
    } else {
        resp.content_length().unwrap_or(entry.size_bytes)
    };

    // Open file for append (resume) or create
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(existing_size > 0)
        .write(true)
        .truncate(existing_size == 0)
        .open(&part_path)
        .await?;

    let mut downloaded = existing_size;
    let mut stream = resp.bytes_stream();
    use futures_lite::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(GrymoireError::Http)?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    file.flush().await?;
    drop(file);

    // Verify size
    let actual_size = tokio::fs::metadata(&part_path).await?.len();
    if actual_size != entry.size_bytes {
        return Err(GrymoireError::Other(format!(
            "size mismatch: expected {} bytes, got {actual_size}",
            entry.size_bytes
        )));
    }

    // Verify SHA256
    let sha_path = part_path.clone();
    let expected = entry.sha256.to_string();
    let valid = tokio::task::spawn_blocking(move || verify_sha256(&sha_path, &expected))
        .await
        .map_err(|e| GrymoireError::Other(format!("sha256 task failed: {e}")))??;

    if !valid {
        // Delete corrupted file
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(GrymoireError::Other("SHA256 verification failed".into()));
    }

    // Rename .part to final
    tokio::fs::rename(&part_path, &dest).await?;

    tracing::info!("downloaded {} to {}", entry.hf_filename, dest.display());
    Ok(dest)
}

/// Verify SHA256 of a file. Runs synchronously (use spawn_blocking for async).
fn verify_sha256(path: &Path, expected_hex: &str) -> Result<bool> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024]; // 1MB buffer
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = format!("{:x}", hasher.finalize());
    Ok(result == expected_hex)
}

/// Delete a downloaded model file.
pub fn delete_model(entry: &CatalogEntry) -> Result<()> {
    let path = model_path(entry)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {
            tracing::info!("deleted model: {}", path.display());
            // Try to remove the parent directory if empty
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GrymoireError::Io(e)),
    }
}

/// Delete any partial download for a model.
pub fn delete_partial(entry: &CatalogEntry) -> Result<()> {
    let dest = model_path(entry)?;
    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().unwrap_or_default().to_string_lossy()
    ));
    match std::fs::remove_file(&part_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GrymoireError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_path_format() {
        let entry = crate::llm_catalog::get_catalog_entry("qwen2.5-1.5b-q4km").unwrap();
        let path = model_path(entry).unwrap();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("Qwen--Qwen2.5-1.5B-Instruct-GGUF"),
            "path should contain slugified repo: {path_str}"
        );
        assert!(
            path_str.ends_with("qwen2.5-1.5b-instruct-q4_k_m.gguf"),
            "path should end with filename: {path_str}"
        );
    }

    #[test]
    fn test_is_model_downloaded_missing() {
        let entry = crate::llm_catalog::get_catalog_entry("smollm2-360m-q8").unwrap();
        // Model won't exist in test environment
        let downloaded = is_model_downloaded(entry).unwrap();
        assert!(!downloaded);
    }
}
