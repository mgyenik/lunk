//! Curated catalog of LLM models available for download.
//!
//! Each entry contains the HuggingFace repo, filename, file size, and SHA256
//! hash for integrity verification. The catalog is static — updates ship with
//! new app versions.

use serde::{Deserialize, Serialize};

/// Chat template format used by the model for instruct prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatTemplate {
    /// `<|im_start|>role\ncontent<|im_end|>` — SmolLM2, Qwen, Phi
    ChatML,
    /// `<|start_header_id|>role<|end_header_id|>\ncontent<|eot_id|>` — Llama 3.x
    Llama3,
    /// `<start_of_turn>role\ncontent<end_of_turn>` — Gemma
    Gemma,
}

/// A model available for download from HuggingFace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Unique slug (e.g., "qwen2.5-1.5b-q4km")
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Short description for the UI
    pub description: &'static str,
    /// HuggingFace repo (e.g., "Qwen/Qwen2.5-1.5B-Instruct-GGUF")
    pub hf_repo: &'static str,
    /// Filename within the repo
    pub hf_filename: &'static str,
    /// File size in bytes
    pub size_bytes: u64,
    /// SHA256 hex digest (from HF lfs.oid) for verification
    pub sha256: &'static str,
    /// Parameter count label (e.g., "1.5B")
    pub param_label: &'static str,
    /// Quantization label (e.g., "Q4_K_M")
    pub quant_label: &'static str,
    /// Context window size
    pub context_size: u32,
    /// Shown as recommended in the UI
    pub recommended: bool,
    /// Minimum RAM in MB to run comfortably
    pub min_ram_mb: u32,
    /// Chat template format for prompt construction
    pub chat_template: ChatTemplate,
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "smollm2-360m-q8",
        name: "SmolLM2 360M",
        description: "Tiny and fast. Runs on any machine. Good for basic title generation.",
        hf_repo: "HuggingFaceTB/SmolLM2-360M-Instruct-GGUF",
        hf_filename: "smollm2-360m-instruct-q8_0.gguf",
        size_bytes: 386_404_992,
        sha256: "48ab3034d0dd401fbc721eb1df3217902fee7dab9078992d66431f09b7750201",
        param_label: "360M",
        quant_label: "Q8_0",
        context_size: 2048,
        recommended: false,
        min_ram_mb: 512,
        chat_template: ChatTemplate::ChatML,
    },
    CatalogEntry {
        id: "qwen2.5-1.5b-q4km",
        name: "Qwen 2.5 1.5B",
        description: "Great balance of speed and quality. Recommended for most users.",
        hf_repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        hf_filename: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        size_bytes: 1_117_320_736,
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        param_label: "1.5B",
        quant_label: "Q4_K_M",
        context_size: 32768,
        recommended: true,
        min_ram_mb: 1200,
        chat_template: ChatTemplate::ChatML,
    },
    CatalogEntry {
        id: "smollm3-3b-q4km",
        name: "SmolLM3 3B",
        description: "HuggingFace's best small model. Strong general-purpose performance.",
        hf_repo: "ggml-org/SmolLM3-3B-GGUF",
        hf_filename: "SmolLM3-Q4_K_M.gguf",
        size_bytes: 1_915_305_312,
        sha256: "8334b850b7bd46238c16b0c550df2138f0889bf433809008cc17a8b05761863e",
        param_label: "3B",
        quant_label: "Q4_K_M",
        context_size: 8192,
        recommended: false,
        min_ram_mb: 2200,
        chat_template: ChatTemplate::ChatML,
    },
    CatalogEntry {
        id: "llama3.2-3b-q4km",
        name: "Llama 3.2 3B",
        description: "Meta's compact model. Broad ecosystem, well-tested.",
        hf_repo: "bartowski/Llama-3.2-3B-Instruct-GGUF",
        hf_filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        size_bytes: 2_019_377_696,
        sha256: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
        param_label: "3B",
        quant_label: "Q4_K_M",
        context_size: 131072,
        recommended: false,
        min_ram_mb: 2400,
        chat_template: ChatTemplate::Llama3,
    },
    CatalogEntry {
        id: "phi4-mini-q4km",
        name: "Phi-4 Mini",
        description: "Microsoft's compact model. Strong reading comprehension. Best for RAG.",
        hf_repo: "unsloth/Phi-4-mini-instruct-GGUF",
        hf_filename: "Phi-4-mini-instruct-Q4_K_M.gguf",
        size_bytes: 2_491_874_272,
        sha256: "88c00229914083cd112853aab84ed51b87bdf6b9ce42f532d8c85c7c63b1730a",
        param_label: "3.8B",
        quant_label: "Q4_K_M",
        context_size: 131072,
        recommended: false,
        min_ram_mb: 2800,
        chat_template: ChatTemplate::ChatML,
    },
    CatalogEntry {
        id: "gemma3-4b-q4km",
        name: "Gemma 3 4B",
        description: "Google's top small model. Excellent quality, larger download.",
        hf_repo: "bartowski/google_gemma-3-4b-it-GGUF",
        hf_filename: "google_gemma-3-4b-it-Q4_K_M.gguf",
        size_bytes: 2_489_758_112,
        sha256: "4996030242583a40aa151ff93f49ed787ac8c25e4120c3ae4588b2e2a7d1ae94",
        param_label: "4B",
        quant_label: "Q4_K_M",
        context_size: 131072,
        recommended: false,
        min_ram_mb: 2800,
        chat_template: ChatTemplate::Gemma,
    },
];

/// Look up a catalog entry by its unique ID.
pub fn get_catalog_entry(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Format a byte count for display (e.g., "1.1 GB", "386 MB").
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_ids_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len(), "duplicate catalog IDs");
    }

    #[test]
    fn test_get_catalog_entry() {
        assert!(get_catalog_entry("qwen2.5-1.5b-q4km").is_some());
        assert!(get_catalog_entry("nonexistent").is_none());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(386_404_992), "386 MB");
        assert_eq!(format_size(1_117_320_736), "1.1 GB");
        assert_eq!(format_size(2_491_874_272), "2.5 GB");
    }

    #[test]
    fn test_recommended_exists() {
        assert!(
            CATALOG.iter().any(|e| e.recommended),
            "at least one model should be recommended"
        );
    }
}
