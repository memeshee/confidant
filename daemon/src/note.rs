//! The note schema — the only shape the model is allowed to produce.
//!
//! Every claim in a note is an [`Claim`]: the text the clinician will read, plus the
//! `quote` from the transcript that supports it. The model never emits offsets; the
//! validator locates the quote and fills `span` (see `validator.rs`). A claim whose quote
//! cannot be located is moved to `unverified` — it is never written into the note.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Claim {
    /// The sentence as it will appear in the note.
    pub text: String,
    /// Verbatim substring of the transcript that supports `text`.
    #[serde(default)]
    pub quote: String,
    /// Filled by the validator, never by the model: byte offsets of `quote` in the transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// The exact transcript characters the span covers, so a UI can highlight without
    /// re-deriving offsets (JS counts UTF-16 units, Rust counts bytes — do not mix them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskFlag {
    /// One of: self_harm, harm_to_other, substances, safeguarding, none_stated.
    pub kind: String,
    /// Rule id that fired, e.g. `rule:denies_si`.
    pub source: String,
    /// The transcript phrase the rule matched.
    pub evidence: String,
}

/// What the model produces (no offsets, no risk flags — those are ours).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DraftNote {
    #[serde(default)]
    pub subjective: Vec<Claim>,
    #[serde(default)]
    pub objective: Vec<Claim>,
    #[serde(default)]
    pub assessment: Vec<Claim>,
    #[serde(default)]
    pub plan: Vec<Claim>,
    #[serde(default)]
    pub next_focus: Vec<Claim>,
}

/// What the API returns: the draft plus everything the guardrail decided.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Note {
    pub id: String,
    pub created_at: String,
    pub model: String,
    pub transcript: String,
    pub draft: DraftNote,
    /// Rule-derived only. The model cannot create, remove or edit these.
    pub risk_flags: Vec<RiskFlag>,
    /// Claims the validator refused: quoted text that is not in the transcript.
    pub unverified: Vec<Claim>,
    pub stats: Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Stats {
    pub claims_total: usize,
    pub claims_verified: usize,
    pub claims_unverified: usize,
    /// char offset pairs the UI highlights, in transcript order.
    pub spans: Vec<Span>,
    pub gen_ms: u64,
    pub verify_ms: u64,
    pub stt_ms: u64,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
    pub tokens_per_sec: f64,
    /// USD per note for this run: 0.0 on a local model. Filled from the model provider.
    pub cost_usd: f64,
}


/// JSON schema handed to the local runtime, so a small model cannot wander off-shape.
/// Loaded from `prompts/note_schema.json` — the same file the eval harness scores against.
pub fn draft_schema() -> serde_json::Value {
    serde_json::from_str(include_str!("../../prompts/note_schema.json"))
        .expect("prompts/note_schema.json must be valid JSON at build time")
}

