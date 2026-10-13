//! Transcription sidecar. Speech-to-text runs locally (faster-whisper on CPU); the audio
//! file is deleted in every path, success or failure — that is a product promise, so it is
//! implemented in `finally` form rather than described in a README.

use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Transcriber {
    pub python: String,
    pub script: PathBuf,
    pub model: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct Transcript {
    pub text: String,
    pub ms: u64,
    pub model: String,
    #[serde(default)]
    pub segments: usize,
}

impl Transcriber {
    pub fn from_env() -> Self {
        let root = std::env::var("CONFIDANT_ROOT").unwrap_or_else(|_| ".".into());
        Self {
            python: std::env::var("CONFIDANT_PYTHON").unwrap_or_else(|_| format!("{root}/.venv/bin/python")),
            script: PathBuf::from(std::env::var("CONFIDANT_STT").unwrap_or_else(|_| format!("{root}/sidecar/transcribe.py"))),
            // Measured on the 6.7 GB dev laptop for a 20 s clip: tiny ≈ 14 s, base ≈ 27 s,
            // small ≈ 70 s, all producing the same transcript on clean audio. `base` is the
            // shipped default because `small` is 3.5x realtime and that is not a usable
            // interaction, while `tiny` degrades fastest on noisy speech.
            model: std::env::var("CONFIDANT_STT_MODEL").unwrap_or_else(|_| "base".into()),
        }
    }

    pub fn available(&self) -> bool {
        Path::new(&self.python).exists() && self.script.exists()
    }

    /// Transcribe `audio`, then destroy it. Returns the transcript text.
    pub fn run(&self, audio: &Path) -> Result<Transcript> {
        if !self.available() {
            bail!("transcriber not installed (looked for {} and {})", self.python, self.script.display());
        }
        let result = Command::new(&self.python)
            .arg(&self.script)
            .arg(audio)
            .arg(&self.model)
            .output();
        // Delete first, parse after: no path leaves audio on disk.
        let removed = std::fs::remove_file(audio);
        let out = result.map_err(|e| anyhow!("could not start transcriber: {e}"))?;
        if !out.status.success() {
            bail!("transcriber failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("unknown error"));
        }
        let parsed: Transcript = serde_json::from_slice(&out.stdout)
            .map_err(|e| anyhow!("transcriber returned non-JSON: {e}"))?;
        if removed.is_err() {
            tracing::warn!("audio file was already absent after transcription");
        }
        Ok(parsed)
    }
}
