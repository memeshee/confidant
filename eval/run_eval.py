#!/usr/bin/env python3
"""Confidant evaluation harness — scores the SHIPPED configuration, not a copy of it.

The system prompts and the JSON schema are read from `prompts/`, the same files compiled into
the daemon, so a change to the product changes the measurement.

Arms
  rules-only        : deterministic template extraction, no model (the honest floor)
  local-<model>     : the local runtime, raw draft (what the model produced by itself)
  local-<model>+g   : the same draft after the guardrail (what actually ships)
  remote-<model>    : any OpenAI-compatible endpoint, line protocol, then the guardrail

Metrics per recap
  fact_recall      : fraction of annotated facts whose keywords all appear in the note text
  distractor_hits  : fraction of planted plausible-but-absent claims the model wrote anyway
  unsupported_rate : fraction of claims whose quote cannot be located in the transcript
  latency_ms, prompt/completion tokens, usd/note

Usage
  python3 eval/run_eval.py --limit 3 --arm rules-only
  python3 eval/run_eval.py --arm local+guard --model gemma3:1b-it-qat
  python3 eval/run_eval.py --arm remote --model google/gemma-3-27b-it --needs-key tokenfactory
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import re
import statistics
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OLLAMA = os.environ.get("CONFIDANT_OLLAMA", "http://127.0.0.1:11434")
SECTIONS = ["subjective", "objective", "assessment", "plan", "next_focus"]


# ── the guardrail, mirrored from daemon/src/validator.rs ────────────────────────────────────
# Kept deliberately identical in rule: normalise to lowercase alphanumerics, collapse spaces,
# then require the quote to be findable. Rust unit tests own the real behaviour; this mirror
# exists only to score the RAW arm, which never passes through the daemon.
def normalise(s: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"[^a-z0-9 ]", "", s.lower())).strip()


def locate(transcript: str, quote: str) -> bool:
    needle = normalise(quote)
    if len(needle) < 12:
        return False
    hay = normalise(transcript)
    if needle in hay:
        return True
    words = needle.split()
    if len(words) < 4:
        return False
    return " ".join(words[:-1]) in hay


# ── corpus ──────────────────────────────────────────────────────────────────────────────────
def load_corpus(limit: int | None) -> list[dict]:
    recaps: list[dict] = []
    for path in sorted(glob.glob(str(ROOT / "eval" / "corpus" / "part-*.json"))):
        data = json.loads(Path(path).read_text())
        recaps.extend(data["recaps"])
    if limit:
        recaps = recaps[:limit]
    return recaps


# ── scoring ─────────────────────────────────────────────────────────────────────────────────
def claims_of(draft: dict) -> list[dict]:
    out = []
    for s in SECTIONS:
        for c in draft.get(s, []) or []:
            if isinstance(c, dict):
                out.append(c)
    return out


def score(recap: dict, draft: dict, transcript: str) -> dict:
    claims = claims_of(draft)
    # Where each fact landed matters: a baseline that dumps the whole transcript into every
    # section would otherwise score a perfect recall while producing no note at all.
    by_section = {
        s: " ".join(((c.get("text") or "") + " " + (c.get("quote") or "")) for c in (draft.get(s) or []) if isinstance(c, dict)).lower()
        for s in SECTIONS
    }
    all_text = " ".join(by_section.values())

    facts = recap.get("facts", [])
    hit_any = sum(1 for f in facts if all(k.lower() in all_text for k in f.get("keywords", [])))
    hit_section = sum(
        1
        for f in facts
        if all(k.lower() in by_section.get(f.get("section", ""), "") for k in f.get("keywords", []))
    )

    dist = recap.get("distractors", [])
    dhit = 0
    for d in dist:
        sig = [w for w in re.findall(r"[a-z]{5,}", d.get("statement", "").lower())][:3]
        if sig and sum(1 for w in sig if w in all_text) >= 2:
            dhit += 1

    unsupported = sum(1 for c in claims if not locate(transcript, c.get("quote") or ""))
    # Only the note's own sentences count as the note; quotes are provenance, not output.
    note_words = len(" ".join((c.get("text") or "") for c in claims).split())
    transcript_words = max(len(transcript.split()), 1)
    return {
        "facts": len(facts),
        "facts_hit": hit_any,
        "facts_in_right_section": hit_section,
        "distractors": len(dist),
        "distractors_hit": dhit,
        "claims": len(claims),
        "claims_unsupported": unsupported,
        "note_words": note_words,
        # 1.0 means "the note is the transcript", which is not a note. Lower is better.
        "brevity": round(note_words / transcript_words, 3),
    }


# ── arms ────────────────────────────────────────────────────────────────────────────────────
def rules_only(transcript: str) -> dict:
    """No model at all: sentence-splitting plus trigger words. The floor the model must beat."""
    plan_triggers = ("we agreed", "will ", "going to", "next week", "homework", "practice")
    assess_triggers = ("i wonder", "my hunch", "seems", "pattern", "hypothes")
    objective_triggers = ("we sat", "asked directly", "reviewed", "completed", "explored")
    draft = {s: [] for s in SECTIONS}
    for raw in re.split(r"(?<=[.!?])\s+", transcript.strip()):
        s = raw.strip()
        if len(s.split()) < 5:
            continue
        low = s.lower()
        if any(t in low for t in plan_triggers):
            section = "plan"
        elif any(t in low for t in assess_triggers):
            section = "assessment"
        elif any(t in low for t in objective_triggers):
            section = "objective"
        else:
            section = "subjective"
        draft[section].append({"text": s, "quote": s})
    return draft


def ollama_raw(system: str, schema: dict, transcript: str, model: str, num_thread: int) -> tuple[dict, dict]:
    body = {
        "model": model,
        "stream": False,
        "format": schema,
        "options": {"temperature": 0.2, "num_ctx": 4096, "num_predict": 1024, "num_thread": num_thread},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": f"RECAP (verbatim, may be messy):\n\n{transcript}"},
        ],
    }
    t0 = time.time()
    req = urllib.request.Request(OLLAMA + "/api/chat", data=json.dumps(body).encode(), headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=900) as r:
        v = json.loads(r.read().decode())
    ms = int((time.time() - t0) * 1000)
    content = v.get("message", {}).get("content", "")
    try:
        draft = json.loads(content)
    except Exception:
        m = re.search(r"\{.*\}", content, re.S)
        draft = json.loads(m.group(0)) if m else {s: [] for s in SECTIONS}
    usage = {
        "ms": ms,
        "prompt_tokens": v.get("prompt_eval_count", 0),
        "completion_tokens": v.get("eval_count", 0),
        "tok_s": round(v.get("eval_count", 0) / max(v.get("eval_duration", 1) / 1e9, 1e-9), 2),
        "usd": 0.0,
    }
    return draft, usage


def remote_raw(system: str, transcript: str, model: str, url: str, key: str, max_tokens: int) -> tuple[dict, dict]:
    body = {
        "model": model,
        "temperature": 0.2,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": f"RECAP (verbatim, may be messy):\n\n{transcript}"},
        ],
    }
    t0 = time.time()
    req = urllib.request.Request(
        url.rstrip("/") + "/chat/completions",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json", "authorization": f"Bearer {key}"},
    )
    with urllib.request.urlopen(req, timeout=900) as r:
        v = json.loads(r.read().decode())
    ms = int((time.time() - t0) * 1000)
    content = v["choices"][0]["message"].get("content") or ""
    draft = {s: [] for s in SECTIONS}
    for line in content.splitlines():
        cleaned = line.strip().lstrip("-*•# ").replace("**", "").replace("`", "")
        parts = cleaned.split("|")
        if len(parts) < 2:
            continue
        head = parts[0].strip().rstrip(":").strip().lower()
        bucket = {
            "s": "subjective", "subj": "subjective", "subjective": "subjective",
            "o": "objective", "obj": "objective", "objective": "objective",
            "a": "assessment", "ass": "assessment", "assessment": "assessment",
            "p": "plan", "plan": "plan",
            "n": "next_focus", "next": "next_focus", "next_focus": "next_focus",
        }.get(head)
        if not bucket:
            continue
        text = parts[1].strip()
        quote = parts[2].strip().strip('"').strip() if len(parts) > 2 else ""
        if text:
            draft[bucket].append({"text": text, "quote": quote})
    u = v.get("usage", {})
    usage = {
        "ms": ms,
        "prompt_tokens": u.get("prompt_tokens", 0),
        "completion_tokens": u.get("completion_tokens", 0),
        "tok_s": round(u.get("completion_tokens", 0) / max(ms / 1000, 1e-9), 2),
        "usd": 0.0,
    }
    return draft, usage


# ── driver ──────────────────────────────────────────────────────────────────────────────────
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", default="local+guard", choices=["rules-only", "local", "local+guard", "remote"])
    ap.add_argument("--model", default="gemma3:1b-it-qat")
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 8)
    ap.add_argument("--needs-key", default=None, help="read the endpoint key from ~/.config/hf26/<name>_key")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    system = (ROOT / "prompts" / "note_system.txt").read_text()
    system_remote = (ROOT / "prompts" / "note_remote.txt").read_text()
    schema = json.loads((ROOT / "prompts" / "note_schema.json").read_text())

    key = ""
    url = os.environ.get("CONFIDANT_REMOTE_URL", "https://api.tokenfactory.nebius.com/v1")
    if args.needs_key:
        key = (Path.home() / ".config" / "hf26" / f"{args.needs_key}_key").read_text().strip()

    recaps = load_corpus(args.limit)
    print(f"corpus: {len(recaps)} recaps · arm={args.arm} model={args.model}")

    rows, t_start = [], time.time()
    for i, recap in enumerate(recaps, 1):
        transcript = recap["transcript"]
        if args.arm == "rules-only":
            draft, usage = rules_only(transcript), {"ms": 0, "prompt_tokens": 0, "completion_tokens": 0, "tok_s": 0.0, "usd": 0.0}
            shipped = draft
        elif args.arm == "local":
            draft, usage = ollama_raw(system, schema, transcript, args.model, args.threads)
            shipped = None
        elif args.arm == "remote":
            draft, usage = remote_raw(system_remote, transcript, args.model, url, key, 1024)
            shipped = None
        else:  # local+guard: run through the shipped daemon so the guardrail is the real one
            body = json.dumps({"transcript": transcript, "model": args.model}).encode()
            req = urllib.request.Request(
                f"http://127.0.0.1:{os.environ.get('CONFIDANT_PORT', '8787')}/api/recap/text",
                data=body,
                headers={"content-type": "application/json", "x-confidant-token": os.environ.get("CONFIDANT_TOKEN", "")},
            )
            t0 = time.time()
            with urllib.request.urlopen(req, timeout=900) as r:
                note = json.loads(r.read().decode())
            usage = {
                "ms": int((time.time() - t0) * 1000),
                "prompt_tokens": note["stats"]["prompt_tokens"],
                "completion_tokens": note["stats"]["output_tokens"],
                "tok_s": round(note["stats"]["tokens_per_sec"], 2),
                "usd": note["stats"]["cost_usd"],
            }
            draft, shipped = note["draft"], note["draft"]
            usage["refused_by_guardrail"] = len(note.get("unverified") or [])

        sc = score(recap, draft, transcript)
        sc.update(usage)
        sc.setdefault("refused_by_guardrail", 0)
        sc["id"] = recap["id"]
        rows.append(sc)
        print(
            f"  [{i}/{len(recaps)}] {recap['id']}  recall {sc['facts_hit']}/{sc['facts']}"
            f"  right-section {sc['facts_in_right_section']}/{sc['facts']}"
            f"  distractors {sc['distractors_hit']}/{sc['distractors']}"
            f"  unsupported {sc['claims_unsupported']}/{sc['claims']}"
            f"  words {sc['note_words']} (x{sc['brevity']})  {sc['ms']} ms",
            flush=True,
        )

    def mean(k):
        vals = [r[k] for r in rows]
        return round(statistics.fmean(vals), 4) if vals else 0.0

    summary = {
        "arm": args.arm,
        "model": args.model,
        "recaps": len(rows),
        "seconds": round(time.time() - t_start, 1),
        "fact_recall": round(sum(r["facts_hit"] for r in rows) / max(sum(r["facts"] for r in rows), 1), 4),
        "fact_section_accuracy": round(
            sum(r["facts_in_right_section"] for r in rows) / max(sum(r["facts"] for r in rows), 1), 4
        ),
        "distractor_rate": round(sum(r["distractors_hit"] for r in rows) / max(sum(r["distractors"] for r in rows), 1), 4),
        "unsupported_rate": round(sum(r["claims_unsupported"] for r in rows) / max(sum(r["claims"] for r in rows), 1), 4),
        "mean_claims_per_note": mean("claims"),
        "mean_refused_by_guardrail": mean("refused_by_guardrail"),
        "brevity": mean("brevity"),
        "median_ms": int(statistics.median([r["ms"] for r in rows])) if rows else 0,
        "mean_tok_s": mean("tok_s"),
        "usd_per_note": round(mean("usd"), 6),
    }
    out = Path(args.out) if args.out else ROOT / "eval" / "results" / f"eval-{args.arm}-{args.model.replace('/', '_')}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"summary": summary, "rows": rows}, indent=2))
    print("\nSUMMARY " + json.dumps(summary, indent=2))
    print(f"written: {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
