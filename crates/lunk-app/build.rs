use std::fs;
use std::io::Read;
use std::path::PathBuf;

/// Model files to download from Hugging Face for the embedding model.
const MODEL_REPO: &str = "Xenova/all-MiniLM-L6-v2";
const MODEL_FILES: &[(&str, &str)] = &[
    ("onnx/model_quantized.onnx", "model_quantized.onnx"),
    ("tokenizer.json", "tokenizer.json"),
    ("config.json", "config.json"),
    ("special_tokens_map.json", "special_tokens_map.json"),
    ("tokenizer_config.json", "tokenizer_config.json"),
];

fn main() {
    // Download embedding model files if not already present
    let model_dir = PathBuf::from("models/all-MiniLM-L6-v2");

    let all_present = MODEL_FILES
        .iter()
        .all(|(_, dest)| model_dir.join(dest).exists());

    if !all_present {
        fs::create_dir_all(&model_dir).expect("failed to create model dir");

        let base_url = format!(
            "https://huggingface.co/{}/resolve/main",
            MODEL_REPO
        );

        for (remote_path, local_name) in MODEL_FILES {
            let dest = model_dir.join(local_name);
            if dest.exists() {
                continue;
            }

            let url = format!("{}/{}", base_url, remote_path);
            println!("cargo:warning=Downloading {url}...");

            let resp = ureq::get(&url)
                .call()
                .unwrap_or_else(|e| panic!("failed to download {url}: {e}"));

            let mut bytes = Vec::new();
            resp.into_reader()
                .read_to_end(&mut bytes)
                .unwrap_or_else(|e| panic!("failed to read {url}: {e}"));

            fs::write(&dest, &bytes)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));

            println!(
                "cargo:warning=Downloaded {} ({} bytes)",
                local_name,
                bytes.len()
            );
        }
    }

    // Only re-run if the model dir changes
    println!("cargo:rerun-if-changed=models/");

    tauri_build::build();
}
