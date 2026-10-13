//! Model clients. Two providers, one invariant.
//!
//! - `Local`: whatever runtime is on loopback (Ollama's native API). Nothing leaves the box.
//! - `Remote`: any OpenAI-compatible endpoint, so a machine that cannot hold a model still
//!   works — optionally a *hosted open-weights* model (Gemma on Nebius Token Factory), which
//!   keeps the "open model" part true even when the compute is not yours.
//!
//! The invariant that does not move: a draft from either provider goes through the
//! deterministic guardrail in `validator.rs` before it is filed. `assert_policy` refuses the
//! one misconfiguration that would make the privacy claim false (local mode pointed at a
//! remote host), and `/api/health` always reports which mode is in force so the UI can say it.

use crate::note::{draft_schema, Claim, DraftNote, Stats};
use anyhow::{anyhow, bail, Result};
use std::time::Instant;

#[derive(Clone, Debug)]
pub enum Provider {
    Local {
        base_url: String,
    },
    Remote {
        url: String,
        key: String,
        /// USD per 1M tokens, in/out. Zero means "billed by the provider, not computed here".
        price_in: f64,
        price_out: f64,
    },
}

#[derive(Clone)]
pub struct Llm {
    pub provider: Provider,
    pub model: String,
    /// Threads handed to the local runtime. Measured on a shared 8-thread laptop as noise
    /// dominated (2.9–10.2 tok/s across 2/4/6/8 threads), so this stays a settable knob
    /// rather than a claimed optimisation.
    pub num_thread: u32,
    pub client: reqwest::Client,
}

/// Shared with the eval harness on purpose: `prompts/note_system.txt` is compiled into the
/// daemon AND read by `eval/run_eval.py`, so the numbers we publish measure the shipped
/// configuration rather than a copy of it. Edit the file, not this line.
const SYSTEM: &str = include_str!("../../prompts/note_system.txt");

/// Remote endpoints generally do not enforce a JSON schema (measured: Nebius Token Factory
/// returns advisory JSON, so a 27B model is free to run away mid-object). The remote path
/// therefore asks for a flat pipe-delimited protocol that models follow far more reliably
/// and that we can parse without depending on the provider's grammar support.
const SYSTEM_REMOTE: &str = include_str!("../../prompts/note_remote.txt");


impl Llm {
    pub fn from_env() -> Self {
        let client = reqwest::Client::new();
        let num_thread = std::env::var("CONFIDANT_THREADS")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4));
        let provider = match std::env::var("CONFIDANT_PROVIDER").unwrap_or_else(|_| "local".into()).as_str() {
            "remote" | "openai" => Provider::Remote {
                url: std::env::var("CONFIDANT_REMOTE_URL")
                    .unwrap_or_else(|_| "https://api.tokenfactory.nebius.com/v1".into()),
                key: std::env::var("CONFIDANT_REMOTE_KEY").unwrap_or_default(),
                price_in: std::env::var("CONFIDANT_REMOTE_PRICE_IN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
                price_out: std::env::var("CONFIDANT_REMOTE_PRICE_OUT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            },
            _ => Provider::Local {
                base_url: std::env::var("CONFIDANT_OLLAMA").unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
            },
        };
        let default_model = match &provider {
            Provider::Remote { .. } => "google/gemma-3-27b-it",
            Provider::Local { .. } => "gemma3:1b-it-qat",
        };
        Self {
            provider,
            model: std::env::var("CONFIDANT_MODEL").unwrap_or_else(|_| default_model.into()),
            num_thread,
            client,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.provider, Provider::Local { .. })
    }

    pub fn label(&self) -> String {
        match &self.provider {
            Provider::Local { base_url } => format!("local {} on {}", self.model, base_url),
            Provider::Remote { url, .. } => format!("remote {} on {}", self.model, url),
        }
    }

    /// Same provider and settings, a different model.
    pub fn with_model(&self, model: String) -> Self {
        Self { model, ..self.clone() }
    }

    /// Local mode *must* address loopback. This is the one misconfiguration that would make
    /// every privacy claim in the README false, so it is a hard refusal, not a warning.
    pub fn assert_policy(&self) -> Result<()> {
        if let Provider::Local { base_url } = &self.provider {
            let ok = base_url.starts_with("http://127.0.0.1")
                || base_url.starts_with("http://localhost")
                || base_url.starts_with("http://[::1]");
            if !ok {
                bail!("refusing to run in local mode against a non-loopback host: {base_url}");
            }
        }
        Ok(())
    }

    pub async fn draft_note(&self, transcript: &str) -> Result<(DraftNote, Stats)> {
        self.assert_policy()?;
        match &self.provider {
            Provider::Local { .. } => self.draft_local(transcript).await,
            Provider::Remote { .. } => self.draft_remote(transcript).await,
        }
    }

    async fn draft_local(&self, transcript: &str) -> Result<(DraftNote, Stats)> {
        let Provider::Local { base_url } = &self.provider else {
            unreachable!("draft_local called for a non-local provider")
        };
        let num_predict: u32 = std::env::var("CONFIDANT_NUM_PREDICT").ok().and_then(|v| v.parse().ok()).unwrap_or(1024);
        let body = serde_json::json!({
            "model": self.model,
            "stream": false,
            "format": draft_schema(),
            "options": {"temperature": 0.2, "num_ctx": 4096, "num_thread": self.num_thread, "num_predict": num_predict, "top_p": 0.9},
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": format!("RECAP (verbatim, may be messy):\n\n{transcript}")}
            ]
        });
        let started = Instant::now();
        let resp = self
            .client
            .post(format!("{base_url}/api/chat"))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("local runtime unreachable at {base_url}: {e}"))?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await.map_err(|e| anyhow!("bad runtime response: {e}"))?;
        if !status.is_success() {
            bail!("local runtime returned {status}: {v}");
        }
        let content = v["message"]["content"].as_str().unwrap_or_default();
        let draft = parse_draft(content)?;
        let eval_count = v["eval_count"].as_u64().unwrap_or(0);
        let eval_ns = v["eval_duration"].as_u64().unwrap_or(0);
        let stats = Stats {
            gen_ms: started.elapsed().as_millis() as u64,
            prompt_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
            output_tokens: eval_count,
            tokens_per_sec: if eval_ns > 0 { eval_count as f64 / (eval_ns as f64 / 1e9) } else { 0.0 },
            cost_usd: 0.0,
            ..Default::default()
        };
        Ok((draft, stats))
    }

    async fn draft_remote(&self, transcript: &str) -> Result<(DraftNote, Stats)> {
        let Provider::Remote { url, key, price_in, price_out } = &self.provider else {
            unreachable!("draft_remote called for a non-remote provider")
        };
        if key.trim().is_empty() {
            bail!("remote mode selected but CONFIDANT_REMOTE_KEY is empty");
        }
        if !url.starts_with("https://") {
            bail!("refusing a non-TLS remote endpoint: {url}");
        }
        let messages = serde_json::json!([
            {"role": "system", "content": SYSTEM_REMOTE},
            {"role": "user", "content": format!("RECAP (verbatim, may be messy):\n\n{transcript}")}
        ]);
        let endpoint = format!("{}/chat/completions", url.trim_end_matches('/'));
        // A cap is mandatory on hosted endpoints: without one the provider's default truncates,
        // and a model free of a grammar constraint is free to keep going.
        let max_tokens: u32 = std::env::var("CONFIDANT_REMOTE_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        let started = Instant::now();
        let resp = self
            .client
            .post(&endpoint)
            .bearer_auth(key)
            .json(&serde_json::json!({
                "model": self.model,
                "temperature": 0.2,
                "max_tokens": max_tokens,
                "messages": messages
            }))
            .send()
            .await
            .map_err(|e| anyhow!("remote endpoint unreachable: {e}"))?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await.map_err(|e| anyhow!("bad remote response: {e}"))?;
        if !status.is_success() {
            bail!("remote endpoint returned {status}: {}", &v.to_string().chars().take(300).collect::<String>());
        }
        let content = v["choices"][0]["message"]["content"].as_str().unwrap_or_default();
        let finish = v["choices"][0]["finish_reason"].as_str().unwrap_or("");
        let draft = parse_lines(content);
        if is_empty_draft(&draft) {
            bail!(
                "remote model returned no parsable note lines (finish_reason={finish}, completion_tokens={}). Head: {}",
                v["usage"]["completion_tokens"],
                &content.chars().take(200).collect::<String>()
            );
        }
        let prompt_tokens = v["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        let gen_ms = started.elapsed().as_millis() as u64;
        let stats = Stats {
            gen_ms,
            prompt_tokens,
            output_tokens,
            tokens_per_sec: if gen_ms > 0 { output_tokens as f64 / (gen_ms as f64 / 1000.0) } else { 0.0 },
            cost_usd: (prompt_tokens as f64 / 1e6) * price_in + (output_tokens as f64 / 1e6) * price_out,
            ..Default::default()
        };
        Ok((draft, stats))
    }

    /// Runtime identity for the footer and the eval harness: mode, model, what is installed,
    /// and whether text leaves the machine in this configuration.
    pub async fn health(&self) -> Result<serde_json::Value> {
        self.assert_policy()?;
        match &self.provider {
            Provider::Local { base_url } => {
                let tags: serde_json::Value = self.client.get(format!("{base_url}/api/tags")).send().await?.json().await?;
                let ps: serde_json::Value = self
                    .client
                    .get(format!("{base_url}/api/ps"))
                    .send()
                    .await?
                    .json()
                    .await
                    .unwrap_or(serde_json::json!({}));
                Ok(serde_json::json!({
                    "mode": "local",
                    "leaves_machine": false,
                    "base_url": base_url,
                    "model": self.model,
                    "threads": self.num_thread,
                    "installed": tags["models"].as_array().map(|m| m.iter().map(|x| x["name"].clone()).collect::<Vec<_>>()).unwrap_or_default(),
                    "loaded": ps["models"],
                }))
            }
            Provider::Remote { url, .. } => Ok(serde_json::json!({
                "mode": "remote",
                "leaves_machine": true,
                "base_url": url,
                "model": self.model,
                "installed": [],
                "warning": "remote mode: the transcript is sent to this endpoint. Intended for machines that cannot hold a local model.",
            })),
        }
    }

    /// Models already on this machine (local) or served by the endpoint (remote).
    pub async fn models(&self) -> Result<serde_json::Value> {
        self.assert_policy()?;
        match &self.provider {
            Provider::Local { base_url } => {
                let tags: serde_json::Value = self.client.get(format!("{base_url}/api/tags")).send().await?.json().await?;
                let list: Vec<serde_json::Value> = tags["models"]
                    .as_array()
                    .map(|ms| {
                        ms.iter()
                            .map(|m| {
                                serde_json::json!({
                                    "name": m["name"],
                                    "size_gb": (m["size"].as_f64().unwrap_or(0.0) / 1e9 * 100.0).round() / 100.0,
                                    "params": m["details"]["parameter_size"],
                                    "quant": m["details"]["quantization_level"],
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(serde_json::json!({ "mode": "local", "models": list, "selected": self.model, "threads": self.num_thread }))
            }
            Provider::Remote { url, key, .. } => {
                let v: serde_json::Value = self
                    .client
                    .get(format!("{}/models", url.trim_end_matches('/')))
                    .bearer_auth(key)
                    .send()
                    .await?
                    .json()
                    .await?;
                let list: Vec<serde_json::Value> = v["data"]
                    .as_array()
                    .map(|ms| ms.iter().map(|m| serde_json::json!({ "name": m["id"] })).collect())
                    .unwrap_or_default();
                Ok(serde_json::json!({ "mode": "remote", "models": list, "selected": self.model }))
            }
        }
    }
}

/// Small models wrap JSON in prose or fences. Extract the object; the guardrail still decides
/// what is *true*, so this only decides what is parseable.
fn parse_draft(content: &str) -> Result<DraftNote> {
    let trimmed = content.trim();
    if let Ok(d) = serde_json::from_str::<DraftNote>(trimmed) {
        return Ok(d);
    }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end > start {
            if let Ok(d) = serde_json::from_str::<DraftNote>(&trimmed[start..=end]) {
                return Ok(d);
            }
        }
    }
    Err(anyhow!(
        "model did not return the note schema; got: {}",
        &trimmed.chars().take(280).collect::<String>()
    ))
}

/// Parse the flat `SECTION | sentence | quote` protocol the remote path asks for.
/// Tolerant by design: bullets, numbering, bold markers and `S:` variants all show up in
/// practice, and a line whose section is unrecognised is dropped rather than guessed at.
pub fn parse_lines(content: &str) -> DraftNote {
    let mut d = DraftNote::default();
    for raw in content.lines() {
        let cleaned = raw.trim().replace("**", "").replace('`', "");
        let cleaned = cleaned.trim_start_matches(['-', '*', '•', '#', ' ']);
        if cleaned.is_empty() {
            continue;
        }
        let mut parts = cleaned.splitn(3, '|');
        let head = parts.next().unwrap_or("").trim().trim_end_matches(':').trim().to_lowercase();
        let bucket = match head.as_str() {
            "s" | "subj" | "subjective" => &mut d.subjective,
            "o" | "obj" | "objective" => &mut d.objective,
            "a" | "ass" | "assessment" => &mut d.assessment,
            "p" | "plan" => &mut d.plan,
            "n" | "next" | "next_focus" | "next focus" => &mut d.next_focus,
            _ => continue,
        };
        let text = parts.next().unwrap_or("").trim().to_string();
        let quote = parts
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        bucket.push(Claim { text, quote, span: None, span_text: None });
    }
    d
}

pub fn is_empty_draft(d: &DraftNote) -> bool {
    d.subjective.is_empty()
        && d.objective.is_empty()
        && d.assessment.is_empty()
        && d.plan.is_empty()
        && d.next_focus.is_empty()
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn parses_the_flat_protocol_with_furniture() {
        let raw = "Here is the note:\n\
                   - S | Dana reported better sleep this week. | she slept maybe six hours most nights\n\
                   **O** | Session focused on the sister relationship. | we sat with that for a while\n\
                   A: | Pattern of being the one who reaches out noted. | that pattern shows up with her mother too\n\
                   P | Will draft the message without sending. | write the message to her sister without sending it\n\
                   Some trailing remark that is not a note line.\n";
        let d = parse_lines(raw);
        assert_eq!(d.subjective.len(), 1);
        assert_eq!(d.objective.len(), 1);
        assert_eq!(d.assessment.len(), 1);
        assert_eq!(d.plan.len(), 1);
        assert!(d.next_focus.is_empty());
        assert_eq!(d.subjective[0].quote, "she slept maybe six hours most nights");
        assert!(!is_empty_draft(&d));
    }

    #[test]
    fn a_prose_only_response_parses_to_nothing() {
        let d = parse_lines("I'm sorry, I can't help with that.");
        assert!(is_empty_draft(&d));
    }
}

#[cfg(test)]
impl Llm {
    fn local_for_test(base_url: &str) -> Self {
        Self {
            provider: Provider::Local { base_url: base_url.into() },
            model: "m".into(),
            num_thread: 4,
            client: reqwest::Client::new(),
        }
    }

    fn remote_for_test() -> Self {
        Self {
            provider: Provider::Remote {
                url: "https://api.example.invalid/v1".into(),
                key: "k".into(),
                price_in: 0.0,
                price_out: 0.0,
            },
            model: "google/gemma-3-27b-it".into(),
            num_thread: 4,
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_remote_host_in_local_mode() {
        let l = Llm::local_for_test("https://api.example.com");
        assert!(l.is_local());
        assert!(l.assert_policy().is_err());
    }

    #[test]
    fn accepts_loopback_in_local_mode() {
        for u in ["http://127.0.0.1:11434", "http://localhost:11434"] {
            assert!(Llm::local_for_test(u).assert_policy().is_ok());
        }
    }

    #[test]
    fn remote_mode_is_allowed_and_says_so() {
        let l = Llm::remote_for_test();
        assert!(!l.is_local());
        assert!(l.assert_policy().is_ok());
        assert!(l.label().starts_with("remote"));
    }

    #[test]
    fn parses_json_wrapped_in_prose_or_fences() {
        let fenced = "```json\n{\"subjective\": [{\"text\": \"t\", \"quote\": \"the exact words spoken\"}], \"objective\": [], \"assessment\": [], \"plan\": [], \"next_focus\": []}\n```";
        assert_eq!(parse_draft(fenced).expect("must extract").subjective.len(), 1);
        let prose = "Sure! Here is the note: {\"subjective\": [], \"objective\": [], \"assessment\": [], \"plan\": [], \"next_focus\": []} Hope that helps.";
        assert!(parse_draft(prose).is_ok());
        assert!(parse_draft("no json here at all").is_err());
    }
}
