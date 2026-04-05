//! LLM-based title generation for archived entries.
//!
//! Uses the loaded LLM model to generate concise, descriptive titles from
//! document text. Falls back to heuristic extraction when no model is available.

use crate::llm_catalog::ChatTemplate;
use crate::llm_engine::LlmEngine;
use crate::titles;

/// Maximum text length to include in the title generation prompt.
/// Generous budget — title is usually in the first few paragraphs but some
/// pages bury it after author metadata. ~3000 chars ≈ 750 tokens, well
/// within even a 4096-token context window.
const MAX_CONTEXT_CHARS: usize = 3000;

/// Maximum tokens to generate for a title.
const MAX_TITLE_TOKENS: u32 = 60;

/// Generate a title using the LLM, or fall back to heuristic extraction.
///
/// This is the main entry point for the save pipeline.
pub fn generate_or_extract_title(
    engine: &LlmEngine,
    extracted_text: &str,
    readable_html: Option<&[u8]>,
    chat_template: Option<ChatTemplate>,
) -> Option<String> {
    // Try LLM first
    if let Some(title) = generate_title(engine, extracted_text, chat_template) {
        return Some(title);
    }

    // Fall back to heuristics
    if let Some(html) = readable_html
        && let Some(title) = titles::title_from_readable_html(html)
    {
        return Some(title);
    }

    titles::title_from_text(extracted_text)
}

/// Generate a title from document text using the LLM.
/// Returns None if the LLM is not loaded, text is too short, or inference fails.
pub fn generate_title(
    engine: &LlmEngine,
    text: &str,
    chat_template: Option<ChatTemplate>,
) -> Option<String> {
    if !engine.is_ready() || text.len() < 50 {
        return None;
    }

    let truncated: String = text.chars().take(MAX_CONTEXT_CHARS).collect();
    let template = chat_template.unwrap_or(ChatTemplate::ChatML);
    let prompt = format_title_prompt(template, &truncated);

    match engine.complete(&prompt, MAX_TITLE_TOKENS) {
        Ok(raw) => {
            let cleaned = clean_llm_title(&raw);
            if cleaned.len() >= 5 && cleaned.len() <= 200 {
                Some(cleaned)
            } else {
                tracing::debug!("LLM title too short/long after cleaning: {cleaned:?}");
                None
            }
        }
        Err(e) => {
            tracing::warn!("LLM title generation failed: {e}");
            None
        }
    }
}

/// Format a title generation prompt for the given chat template.
fn format_title_prompt(template: ChatTemplate, text: &str) -> String {
    let system = "You extract or generate document titles. Rules:\n\
        - If the document has a clear title (heading, paper title, datasheet name), use it exactly\n\
        - Only generate a summary title if the document has no clear title\n\
        - Preserve part numbers (MP6002, AD9446, LT1533), application note numbers (AN133), and model numbers\n\
        - Output ONLY the title, nothing else. No quotes, no explanation\n\
        - Maximum 15 words";
    let user = format!("What is the title of this document?\n\n{text}");

    match template {
        ChatTemplate::ChatML => {
            format!(
                "<|im_start|>system\n{system}<|im_end|>\n\
                 <|im_start|>user\n{user}<|im_end|>\n\
                 <|im_start|>assistant\n"
            )
        }
        ChatTemplate::Llama3 => {
            format!(
                "<|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>\
                 <|start_header_id|>user<|end_header_id|>\n\n{user}<|eot_id|>\
                 <|start_header_id|>assistant<|end_header_id|>\n\n"
            )
        }
        ChatTemplate::Gemma => {
            format!(
                "<start_of_turn>user\n{system}\n\n{user}<end_of_turn>\n\
                 <start_of_turn>model\n"
            )
        }
    }
}

/// Clean up LLM output: strip quotes, take first line, normalize.
fn clean_llm_title(raw: &str) -> String {
    let mut t = raw.trim().to_string();

    // Take only the first line
    if let Some(pos) = t.find('\n') {
        t = t[..pos].to_string();
    }

    // Strip surrounding quotes
    if (t.starts_with('"') && t.ends_with('"'))
        || (t.starts_with('\'') && t.ends_with('\''))
    {
        t = t[1..t.len() - 1].to_string();
    }

    // Strip markdown bold
    if t.starts_with("**") && t.ends_with("**") {
        t = t[2..t.len() - 2].to_string();
    }

    // Apply the existing title cleaner (strips site names, whitespace, etc.)
    titles::clean_title(&t)
}

/// Get the chat template for the active model, if known.
pub fn active_chat_template(engine: &LlmEngine) -> Option<ChatTemplate> {
    let model_id = engine.active_model_id()?;
    let entry = crate::llm_catalog::get_catalog_entry(&model_id)?;
    Some(entry.chat_template)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chatml_prompt() {
        let prompt = format_title_prompt(ChatTemplate::ChatML, "some text");
        assert!(prompt.starts_with("<|im_start|>system"));
        assert!(prompt.contains("some text"));
        assert!(prompt.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn test_format_llama3_prompt() {
        let prompt = format_title_prompt(ChatTemplate::Llama3, "some text");
        assert!(prompt.contains("<|start_header_id|>"));
        assert!(prompt.contains("some text"));
    }

    #[test]
    fn test_format_gemma_prompt() {
        let prompt = format_title_prompt(ChatTemplate::Gemma, "some text");
        assert!(prompt.contains("<start_of_turn>"));
        assert!(prompt.contains("some text"));
    }

    #[test]
    fn test_clean_llm_title_basic() {
        assert_eq!(clean_llm_title("  MP6002 Datasheet  "), "MP6002 Datasheet");
    }

    #[test]
    fn test_clean_llm_title_quoted() {
        assert_eq!(
            clean_llm_title("\"MP6002 Step-Down Converter\""),
            "MP6002 Step-Down Converter"
        );
    }

    #[test]
    fn test_clean_llm_title_multiline() {
        assert_eq!(
            clean_llm_title("Good Title\nSome extra explanation"),
            "Good Title"
        );
    }

    #[test]
    fn test_clean_llm_title_markdown_bold() {
        assert_eq!(
            clean_llm_title("**Important Document Title**"),
            "Important Document Title"
        );
    }

    #[test]
    fn test_generate_title_no_model() {
        let engine = LlmEngine::new().unwrap();
        assert!(generate_title(&engine, "some text here for testing", None).is_none());
    }

    #[test]
    fn test_generate_title_short_text() {
        let engine = LlmEngine::new().unwrap();
        assert!(generate_title(&engine, "short", None).is_none());
    }
}
