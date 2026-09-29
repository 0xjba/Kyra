use std::path::{Path, PathBuf};

pub(crate) fn model_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (home.join(".cache/huggingface"), "Hugging Face models"),
        (home.join(".ollama/models"), "Ollama models"),
        (home.join(".cache/lm-studio"), "LM Studio models"),
    ]
}

pub(crate) fn cache_paths(home: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(home.join("Library/Caches/com.apple.coreml"), "CoreML cache")]
}
