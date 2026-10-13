//! The guardrail: "the model suggests, code decides."
//!
//! Two jobs, both deterministic and both testable without a model:
//!  1. locate every claim's quote in the transcript (normalised, char-offset mapped);
//!     anything that cannot be found verbatim-ish is refused into `unverified`.
//!  2. derive risk flags from explicit clinician phrases by rule — the model is never
//!     asked for them and cannot produce one.

use crate::note::{Claim, DraftNote, RiskFlag, Span, Stats};

/// Lowercase alphanumeric projection of `s`, with a map back to the original char index.
fn normalise(s: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut map = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
                map.push(i);
            }
        } else if ch.is_whitespace() && !out.ends_with(' ') {
            out.push(' ');
            map.push(i);
        }
    }
    (out, map)
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Locate `quote` in `transcript`. Exact after normalisation, else a prefix-trimmed retry
/// (models pad quotes with ellipses and casing). Returns byte span in the ORIGINAL transcript.
pub fn locate(transcript: &str, quote: &str) -> Option<Span> {
    let (hay, hay_map) = normalise(transcript);
    let (needle, _) = normalise(quote);
    if needle.trim().len() < 12 {
        return None;
    }
    let found = hay
        .find(needle.trim())
        .map(|b| (b, b + needle.trim().len()))
        .or_else(|| {
            // retry on the longest clean run of words, for trailing "..." / bracketed asides
            let words: Vec<&str> = needle.split_whitespace().collect();
            if words.len() < 4 {
                return None;
            }
            let core = words[..words.len() - 1].join(" ");
            hay.find(&core).map(|b| (b, b + core.len()))
        })?;
    let (ns, ne) = found;
    let start_char = *hay_map.get(ns)?;
    let end_char = *hay_map.get(ne.saturating_sub(1))? + 1;
    Some(Span {
        start: char_to_byte(transcript, start_char),
        end: char_to_byte(transcript, end_char),
    })
}

fn verify_section(claims: Vec<Claim>, transcript: &str, refused: &mut Vec<Claim>, spans: &mut Vec<Span>) -> Vec<Claim> {
    let mut kept = Vec::new();
    for mut c in claims {
        if c.text.trim().is_empty() {
            continue;
        }
        match locate(transcript, &c.quote) {
            Some(sp) => {
                c.span_text = Some(transcript[sp.start..sp.end].to_string());
                c.span = Some(sp.clone());
                spans.push(sp);
                kept.push(c);
            }
            None => {
                // Refuse rather than repair: an unsupported sentence must never be filed.
                c.span = None;
                c.span_text = None;
                refused.push(c);
            }
        }
    }
    kept
}

struct Rule {
    id: &'static str,
    kind: &'static str,
    phrases: &'static [&'static str],
}

/// Explicit, first-person clinical phrases only. Deliberately small and auditable: a missed
/// flag is the clinician's to notice, an invented one is ours.
const RULES: &[Rule] = &[
    Rule { id: "denies_si", kind: "self_harm", phrases: &["denies si", "denies suicidal", "no si", "no suicidal ideation", "denied si"] },
    Rule { id: "reports_si", kind: "self_harm", phrases: &["reports si", "endorses si", "suicidal ideation", "thoughts of suicide", "wants to die"] },
    Rule { id: "reports_self_harm", kind: "self_harm", phrases: &["self-harm", "self harm", "cutting", "cut herself", "cut himself"] },
    Rule { id: "harm_to_other", kind: "harm_to_other", phrases: &["hurt someone", "harm to others", "homicidal", "threatened to hurt"] },
    Rule { id: "substances", kind: "substances", phrases: &["drinking daily", "using cocaine", "opioid", "relapsed", "relapse", "alcohol daily", "cannabis daily"] },
    Rule { id: "safeguarding", kind: "safeguarding", phrases: &["child protection", "safeguarding", "mandated report", "abuse disclosure", "disclosed abuse"] },
];

pub fn risk_flags(transcript: &str, refused: &[Claim]) -> Vec<RiskFlag> {
    let (hay, _) = normalise(transcript);
    let mut flags = Vec::new();
    for r in RULES {
        for p in r.phrases {
            if hay.contains(p) {
                flags.push(RiskFlag {
                    kind: r.kind.to_string(),
                    source: format!("rule:{}", r.id),
                    evidence: (*p).to_string(),
                });
                break;
            }
        }
    }
    // A refusal that looks like a risk statement is itself worth surfacing to the clinician.
    for c in refused {
        let l = c.text.to_lowercase();
        if RULES.iter().any(|r| r.phrases.iter().any(|p| l.contains(p))) && !flags.iter().any(|f| f.kind == "self_harm") {
            flags.push(RiskFlag {
                kind: "self_harm".into(),
                source: "refused_claim_review".into(),
                evidence: c.text.clone(),
            });
        }
    }
    if flags.is_empty() {
        flags.push(RiskFlag {
            kind: "none_stated".into(),
            source: "rule:no_risk_phrase".into(),
            evidence: String::new(),
        });
    }
    flags
}

pub struct Verified {
    pub draft: DraftNote,
    pub unverified: Vec<Claim>,
    pub risk_flags: Vec<RiskFlag>,
    pub stats_partial: Stats,
}

pub fn verify(transcript: &str, draft: DraftNote) -> Verified {
    let mut refused = Vec::new();
    let mut spans = Vec::new();
    let total = draft.subjective.len()
        + draft.objective.len()
        + draft.assessment.len()
        + draft.plan.len()
        + draft.next_focus.len();
    let out = DraftNote {
        subjective: verify_section(draft.subjective, transcript, &mut refused, &mut spans),
        objective: verify_section(draft.objective, transcript, &mut refused, &mut spans),
        assessment: verify_section(draft.assessment, transcript, &mut refused, &mut spans),
        plan: verify_section(draft.plan, transcript, &mut refused, &mut spans),
        next_focus: verify_section(draft.next_focus, transcript, &mut refused, &mut spans),
    };
    let verified = out.subjective.len()
        + out.objective.len()
        + out.assessment.len()
        + out.plan.len()
        + out.next_focus.len();
    let flags = risk_flags(transcript, &refused);
    spans.sort_by_key(|s| s.start);
    Verified {
        draft: out,
        unverified: refused,
        risk_flags: flags,
        stats_partial: Stats {
            claims_total: total,
            claims_verified: verified,
            claims_unverified: total - verified,
            spans,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Claim;

    fn claim(t: &str, q: &str) -> Claim {
        Claim { text: t.into(), quote: q.into(), span: None, span_text: None }
    }

    #[test]
    fn locates_a_real_quote_with_punctuation_and_case() {
        let t = "She said the panic attacks came back after the move, mainly at night.";
        let sp = locate(t, "panic attacks came back after the move").expect("must locate");
        assert_eq!(&t[sp.start..sp.end], "panic attacks came back after the move");
    }

    #[test]
    fn refuses_an_invented_quote() {
        let t = "We talked about her sleep and the new job.";
        assert!(locate(t, "client reported taking 40mg fluoxetine daily").is_none());
    }

    #[test]
    fn tolerate_trailing_ellipsis_from_small_models() {
        let t = "She wants to try the breathing exercise before the presentation next week.";
        assert!(locate(t, "wants to try the breathing exercise ...").is_some());
    }

    #[test]
    fn a_refused_claim_is_never_in_the_note() {
        let t = "Session focused on sleep hygiene and the new job. She slept five hours a night, and reports taking 40mg fluoxetine.";
        let d = DraftNote {
            subjective: vec![
                claim("Slept five hours", "slept five hours a night"),
                claim("Reports 200mg of sertraline", "reports taking 200mg of sertraline"),
            ],
            ..Default::default()
        };
        let v = verify(t, d);
        assert_eq!(v.draft.subjective.len(), 1);
        assert_eq!(v.unverified.len(), 1);
        assert_eq!(v.stats_partial.claims_unverified, 1);
    }

    #[test]
    fn risk_flags_come_from_rules_not_the_model() {
        let t = "Asked directly, she denies SI and there is no plan.";
        let v = verify(t, DraftNote::default());
        assert!(v.risk_flags.iter().any(|f| f.source == "rule:denies_si"));
    }

    #[test]
    fn empty_transcript_yields_none_stated_and_no_claims() {
        let v = verify("", DraftNote::default());
        assert_eq!(v.risk_flags.len(), 1);
        assert_eq!(v.risk_flags[0].kind, "none_stated");
        assert_eq!(v.stats_partial.claims_verified, 0);
    }
}
