//! Export renderers. Markdown for the record; a print-ready HTML sheet the clinician can
//! save as PDF from the browser — no PDF crate, no cloud converter, no data leaving.

use crate::note::Note;

fn label(kind: &str) -> &str {
    match kind {
        "self_harm" => "self-harm / suicidal ideation",
        "harm_to_other" => "risk to others",
        "substances" => "substances",
        "safeguarding" => "safeguarding",
        _ => "no risk phrase stated",
    }
}

fn section_md(title: &str, claims: &[crate::note::Claim]) -> String {
    if claims.is_empty() {
        return format!("### {title}\n\n_Not stated in the recap._\n\n");
    }
    let mut s = format!("### {title}\n\n");
    for c in claims {
        s.push_str(&format!("- {}\n", c.text.trim()));
    }
    s.push('\n');
    s
}

pub fn to_markdown(n: &Note) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Session note — {}\n\n", &n.created_at[..19.min(n.created_at.len())]));
    out.push_str(&format!(
        "Model: `{}` · {} of {} sentences traced to what you said · {} refused as unsupported\n\n",
        n.model, n.stats.claims_verified, n.stats.claims_total, n.stats.claims_unverified
    ));
    out.push_str(&format!("**Risk flags (rule-derived):** {}\n\n", n.risk_flags.iter().map(|f| format!("{} _({}, matched “{}”)_", label(&f.kind), f.source, f.evidence)).collect::<Vec<_>>().join("; ")));
    for (t, c) in [
        ("Subjective", &n.draft.subjective),
        ("Objective", &n.draft.objective),
        ("Assessment", &n.draft.assessment),
        ("Plan", &n.draft.plan),
        ("Next session focus", &n.draft.next_focus),
    ] {
        out.push_str(&section_md(t, c));
    }
    if !n.unverified.is_empty() {
        out.push_str("### Refused by the guardrail (not in your recap — review before use)\n\n");
        for c in &n.unverified {
            out.push_str(&format!("- ~~{}~~ — no supporting words found\n", c.text.trim()));
        }
        out.push('\n');
    }
    out.push_str("---\n\n");
    out.push_str("**Recap as spoken (source of every sentence above):**\n\n> ");
    out.push_str(&n.transcript.replace('\n', "\n> "));
    out.push('\n');
    out
}

pub fn to_html(n: &Note) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut body = String::new();
    body.push_str(&format!("<h1>Session note <span class=\"m\">{}</span></h1>", esc(&n.created_at[..19.min(n.created_at.len())])));
    body.push_str(&format!(
        "<p class=\"meta\">{} of {} sentences traced to the recap · {} refused · model {} · 0 network calls</p>",
        n.stats.claims_verified, n.stats.claims_total, n.stats.claims_unverified, esc(&n.model)
    ));
    body.push_str("<div class=\"flags\">");
    for f in &n.risk_flags {
        body.push_str(&format!("<span class=\"flag {}\">{}</span>", esc(&f.kind), esc(label(&f.kind))));
    }
    body.push_str("</div>");
    for (t, c) in [
        ("Subjective", &n.draft.subjective),
        ("Objective", &n.draft.objective),
        ("Assessment", &n.draft.assessment),
        ("Plan", &n.draft.plan),
        ("Next session focus", &n.draft.next_focus),
    ] {
        body.push_str(&format!("<h2>{t}</h2>"));
        if c.is_empty() {
            body.push_str("<p class=\"none\">Not stated in the recap.</p>");
        } else {
            body.push_str("<ul>");
            for cl in c {
                let quote = cl
                    .span
                    .as_ref()
                    .map(|s| esc(&n.transcript[s.start..s.end]))
                    .unwrap_or_else(|| esc(&cl.quote));
                body.push_str(&format!("<li>{}<span class=\"src\">{}</span></li>", esc(&cl.text.trim()), quote));
            }
            body.push_str("</ul>");
        }
    }
    if !n.unverified.is_empty() {
        body.push_str("<h2>Refused by the guardrail</h2><ul class=\"refused\">");
        for c in &n.unverified {
            body.push_str(&format!("<li>{}<span class=\"src\">no supporting words in the recap</span></li>", esc(&c.text.trim())));
        }
        body.push_str("</ul>");
    }
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Session note</title>
<style>
 :root{{--paper:#fdfaf5;--ink:#171310;--accent:#c25a44;--muted:#8a8078}}
 *{{box-sizing:border-box}}
 body{{margin:0;background:var(--paper);color:var(--ink);font:16px/1.6 "Source Serif 4",Georgia,serif}}
 main{{max-width:44rem;margin:0 auto;padding:3rem 1.5rem 6rem}}
 h1{{font:600 1.9rem/1.2 "Inter Tight",system-ui,sans-serif;letter-spacing:-.02em;margin:0 0 .4rem}}
 h2{{font:600 1.05rem/1.3 "Inter Tight",system-ui,sans-serif;letter-spacing:.02em;text-transform:uppercase;color:var(--muted);margin:2.6rem 0 .6rem;border-top:1px solid #e7ded1;padding-top:1.4rem}}
 .m,.meta{{font:400 .82rem/1.5 "IBM Plex Mono",ui-monospace,monospace;color:var(--muted)}}
 ul{{list-style:none;padding:0;margin:0}}
 li{{margin:0 0 .9rem}}
 .src{{display:block;font:400 .78rem/1.5 "IBM Plex Mono",ui-monospace,monospace;color:var(--accent);margin-top:.15rem}}
 .refused li{{color:var(--muted);text-decoration:line-through}}
 .none{{color:var(--muted);font-style:italic}}
 .flags{{margin:1.2rem 0 0}}
 .flag{{display:inline-block;border:1px solid var(--accent);color:var(--accent);border-radius:999px;padding:.15rem .7rem;font:500 .75rem/1.4 "Inter Tight",system-ui,sans-serif;margin-right:.4rem}}
 @media print{{body{{background:#fff}} .src{{color:#666}}}}
</style></head><body><main>{body}</main></body></html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::{Claim, DraftNote, RiskFlag, Stats};

    fn note() -> Note {
        Note {
            id: "n".into(),
            created_at: "2026-10-13T10:00:00Z".into(),
            model: "gemma3:1b-it-qat".into(),
            transcript: "She said the panic came back at night.".into(),
            draft: DraftNote { subjective: vec![Claim { text: "Panic returned at night.".into(), quote: "the panic came back at night".into(), span: None, span_text: None }], ..Default::default() },
            risk_flags: vec![RiskFlag { kind: "none_stated".into(), source: "rule:no_risk_phrase".into(), evidence: String::new() }],
            unverified: vec![Claim { text: "Reports 40mg fluoxetine".into(), quote: "reports 40mg".into(), span: None, span_text: None }],
            stats: Stats { claims_total: 2, claims_verified: 1, claims_unverified: 1, ..Default::default() },
        }
    }

    #[test]
    fn markdown_carries_the_refusal_and_the_source() {
        let md = to_markdown(&note());
        assert!(md.contains("Refused by the guardrail"));
        assert!(md.contains("Reports 40mg fluoxetine"));
        assert!(md.contains("She said the panic came back at night."));
    }

    #[test]
    fn html_is_self_contained_and_escaped() {
        let h = to_html(&note());
        assert!(h.starts_with("<!doctype html>"));
        assert!(h.contains("Panic returned at night."));
        assert!(!h.contains("<script"));
    }
}
