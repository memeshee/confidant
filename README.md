# Confidant

**Session notes that never leave the room.** A clinician speaks a twenty-second recap after a
session; a small open-weights model running on her own laptop drafts the structured note, and
every sentence in it is traceable to the words she actually said. No cloud, no recording, no
per-note cost.

> This is an entry in the [Hacktoberfest Open-Source AI Challenge: Week 2 — Side
> Hustle](https://dev.to/challenges/hacktoberfest-week2-2026-10-12). Categories claimed:
> **Best Use of Render**, **Best Use of Gemma**, **Best Use of a .tech Domain**.
> Two further categories (**Tinker** fine-tuning, **Sentry agent tracing**) are described in the
> post as the measured next step and are *not* claimed here, because a claim without a run
> behind it is exactly what this project refuses to file.

## The problem

A solo therapist finishes a session and has to write it up. Two options exist today:

| Option | What it costs | Why it fails |
|---|---|---|
| Type it at 9pm | ~20 minutes a day, every day | Detail is lost; it is the last unpaid hour of the day |
| A cloud "AI scribe" | $69–114 / month | It **records the session**, and clients do not consent to that |

The second option is not a pricing problem. It is a consent problem, and it is why a closed API
cannot sell into this market: the product requires shipping a client's words to somebody else's
server. A local open-weights model is not a cheaper version of the cloud tool. It is the only
version of this product that can exist.

## What it does

1. **Talk, don't type.** Record a messy recap (or paste text). Speech-to-text runs locally.
2. **The note appears already structured** — Subjective / Objective / Assessment / Plan / Next
   focus — drafted by a small model on the machine.
3. **Every sentence carries its source.** Hover a sentence and the exact transcript span that
   supports it highlights in the side rail.
4. **Anything unsupported is refused.** A deterministic guardrail locates each sentence's
   supporting quote in the recap; a sentence whose words cannot be found is **not filed** — it
   is shown struck through under "refused by the guardrail."
5. **Risk flags are rules, not model output.** A small auditable rule set over the clinician's
   own words decides the risk chips. The model is never asked for a risk level.
6. **Export** to Markdown or a print-ready sheet (browser → PDF), and a local SQLite history of
   sessions.

## Innovation — what is newly possible

Not "it uses AI". Three specific things:

- **Provenance as a primitive, not a feature.** The note is a *view* over the transcript: every
  claim stores the verbatim span that supports it, and the UI can show it. Grounding is not a
  prompt instruction here, it is a data-model invariant.
- **A refusal path.** Unsupported sentences are *unrepresentable* in a filed note, so the model's
  failure mode is "this line is missing and here is what it tried to write" rather than "a
  plausible-sounding clinical sentence that nobody said."
- **Model choice on the clinician's own hardware.** `/api/models` lists whatever is installed
  with real sizes, and the same code path runs a 1B or a 12B. This laptop (6.7 GB RAM) measured
  the trade in public: 1B local = 3 sentences in 56 s; 4B local = 0.26 tok/s (unusable, swap);
  27B hosted = 16 sentences in 10 s but the transcript leaves the machine.

## Architecture

```
microphone ─► faster-whisper (local, CPU) ─► transcript  [audio deleted]
                      │
      ┌───────────────┴───────────────────────────────────────────┐
      │ local mode (default)        remote mode (opt-in, weak HW) │
      │ Ollama · Gemma 3 1B QAT     any OpenAI-compatible endpoint │
      │ 127.0.0.1 only, enforced    e.g. hosted open-weights Gemma │
      └───────────────┬───────────────────────────────────────────┘
                      ▼
        deterministic guardrail (Rust, local, always)
        span lookup · field whitelist · rule-derived risk flags
                      ▼
        SQLite on the clinician's disk ─► note · Markdown/HTML export
                      ▼
        Sentry: stage, latency, tokens, cost, ids — never clinical text
```

- **Backend: Rust** (Axum + tokio + serde + rusqlite), one binary, binds `127.0.0.1` only.
  Every mutating route needs a run token injected into the page the daemon serves, so a random
  website cannot post a transcript into the local store.
- **Frontend: Next.js (App Router) + TypeScript + Tailwind, exported statically** and served by
  the daemon. Stated deviation from the "Next.js on Vercel" default: the product's whole claim
  is that it works with the wifi off, so the app cannot live on a host. Vercel serves only the
  public landing page for the `.tech` domain.
- **Inference**: Ollama on loopback. `assert_policy()` **refuses** any base URL that is not
  loopback in local mode — a hard error, not a warning, because that is the single
  misconfiguration that would make this README's privacy claims false.
- **Shared prompts**: `prompts/note_system.txt` and `prompts/note_schema.json` are compiled into
  the daemon *and* read by the eval harness, so the published numbers measure the shipped
  configuration rather than a copy of it.

## Verification — what is real, what is measured, what is not

| Claim | Status |
|---|---|
| Runs on CPU with no network | **Real.** Measured on the dev laptop (8 threads, 6.7 GB, no GPU). |
| Local mode refuses non-loopback hosts | **Real.** `pipeline::assert_policy`, unit-tested (`refuses_a_remote_host_in_local_mode`). |
| Unsupported sentences are refused | **Real.** `validator::locate`, 6 unit tests including a fabricated medication dose. |
| Risk flags are rule-derived | **Real.** The model's output type has no risk field at all. |
| Audio is deleted after transcription | **Real, observed.** `POST /api/recap/audio` ran the whole path (HTTP 200, local STT in 26 s, note drafted) and `/tmp/confidant-audio` was empty afterwards. Implemented in `transcribe.rs`, where deletion happens before the result is parsed. |
| Sentry receives no clinical text | **Real by construction.** `trace.rs` has no API that accepts text. |
| Evaluation numbers | **Measured** by `eval/run_eval.py` on a fully **synthetic** corpus (no real client data). Committed at `eval/results/`. |
| The corpus | **Synthetic.** 90 recaps (760 annotated facts, 427 planted distractors) written for this project, labelled synthetic in the submission post. |
| Speech-to-text | **Real** (faster-whisper on CPU, `base` model by default). Measured on a 20 s clip: tiny ≈ 14 s, base ≈ 27 s, small ≈ 70 s, identical transcripts on clean audio. Model weights are a one-time 680 MB download into the local HF cache. |
| Remote mode | **Real but opt-in**, and it says so: the transcript leaves the machine. Intended for hardware that cannot hold a model. |
| Hosted 27B comparison | **Real measurement**, and the post reports its failure mode too (the endpoint does not enforce JSON schemas, so the remote path uses a flat line protocol). |

## Running it

```bash
# 1. runtime + model (no sudo needed: user-space tarball)
curl -L -o ollama.tar.zst https://github.com/ollama/ollama/releases/download/v0.40.3/ollama-linux-amd64.tar.zst
tar --zstd -xf ollama.tar.zst && ./bin/ollama serve &
./bin/ollama pull gemma3:1b-it-qat          # 1.0 GB

# 2. speech-to-text sidecar
#    NOTE: faster-whisper declares `av>=11` unbounded and a fresh install pulls av 19, which
#    crashes it (`TypeError: open() got an unexpected keyword argument 'metadata_errors'`).
#    Pin av to 16.1.0; see .venv/INSTALL_NOTES.md for the measurements behind this.
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python faster-whisper "av==16.1.0"

# 3. build the UI and the daemon
cd ui && npm install && npm run build && cd ..
cargo build --release

# 4. run (binds 127.0.0.1:8791, prints the token-bearing URL)
CONFIDANT_ROOT=$PWD CONFIDANT_PORT=8791 ./target/release/confidant-daemon
```

Configuration (all optional, all defaulted): `CONFIDANT_MODEL`, `CONFIDANT_OLLAMA`,
`CONFIDANT_THREADS`, `CONFIDANT_DB`, `CONFIDANT_PORT` (or the platform's `PORT`),
`CONFIDANT_BIND`, `CONFIDANT_STT_MODEL`, `CONFIDANT_PYTHON`, `CONFIDANT_PROVIDER=local|remote`,
`CONFIDANT_REMOTE_URL`, `CONFIDANT_REMOTE_KEY`, `CONFIDANT_REMOTE_MAX_TOKENS`, `CONFIDANT_DEMO`,
`CONFIDANT_DEMO_DIR`, `SENTRY_DSN`.

## Public demo (Render) — a read-only replay

<https://confidant-demo.onrender.com> · service `srv-db73srid0e5s73avf6i0` (free plan, Docker
runtime, Oregon) · built from the `Dockerfile` at the repo root.

The public deployment deliberately **cannot take input**. It runs the daemon with
`CONFIDANT_DEMO=1`, which:

- loads `demo/note.json` and `demo/note-b.json` — notes produced by real runs of this pipeline
  against a local model (`gemma3:1b-it-qat` on loopback; the second run's synthetic input is
  committed at `demo/recap-b.txt`) — plus the newest `eval/results/*.json` at startup;
- serves them at `GET /api/demo` and renders them as a replay: a banner that says so, a switcher
  between the two recorded runs, and the evaluation table rendered from the committed results;
- answers **every** POST, PATCH and DELETE with `403` from a middleware that runs before any
  handler, so nobody can type a client's words into a page that could store them. `cargo test`
  drives the real router and asserts exactly that.

```bash
curl -s https://confidant-demo.onrender.com/api/demo | head -c 120      # the replay payload
curl -s -o /dev/null -w '%{http_code}\n' -X POST \
  https://confidant-demo.onrender.com/api/recap/text \
  -H 'content-type: application/json' -d '{"transcript":"client words"}'  # 403, by design
```

Free instances spin down after ~15 minutes idle; the first request after that can take ~50 s.

`docs/confidant-demo.mp4` (24 s) shows the deployed page, the recorded `403`, and the evaluation
table; `docs/screenshots/` holds the stills. The clip is reproducible — it renders from real
screenshots and recorded output via the Remotion project in `video/` (`cd video && npm i && npm run render`).

## Demo flow (what a judge should try first)

1. Open the app with the **wifi off** and press *Record instead*.
2. Talk for twenty seconds about a session the way you would to a colleague.
3. Stop. The note drafts on the machine; each line shows the span it came from.
4. Look for the line that is struck through: that is the guardrail refusing something the model
   invented.
5. Export to print-ready HTML, then check the footer: `0.0000 USD/note`, tokens/sec, model name.

## Testing

```bash
cargo test                 # 18 tests: guardrail, provider policy, exports, store, line parser, demo mode
python3 eval/run_eval.py --arm rules-only            # baseline, no model
python3 eval/run_eval.py --arm local+guard --limit 4  # shipped path via the daemon
scripts/cleanup.sh         # dry run: lists everything this project added to the machine
```

## Security and reliability

- **Threat model**: leaked client text, accidental audio retention, prompt injection via
  transcript content, model-invented risk statements, supply chain, judge-visible demo data.
- Transcript content is treated as **untrusted data**, never as instructions; the drafting call
  has no tools and cannot act.
- The note generator cannot write a risk flag — the type it returns has no such field.
- Localhost-only binding, an injected run token for writes, TLS-only remote mode, and no
  secrets in the repo (`.gitignore` covers `.env`, keys, and the model cache).
- Failure states are handled, not hoped over: model missing, low RAM, microphone denied,
  transcriber absent, model returns prose instead of the schema, hosted endpoint truncating.

## Limitations and roadmap

- **A 1B model is thin, and its quotes are unreliable.** In one real run on a synthetic 180-word
  recap it produced 7 sentences and **3 were refused** because the model had *paraphrased* the
  text it presented as a quote (`"We will continue to watch takeoff videos for ten minutes a
  day."` was not in the recap). The verbatim evidence is committed at
  `docs/evidence/refusal-example.json`.
- **The guardrail has a false-negative rate.** It trades recall for precision: a sentence whose
  content is true but whose supporting quote was paraphrased gets refused along with the
  invented ones. The clinician sees it struck through and can retype it; the alternative is
  filing a sentence nobody said. The eval table quantifies that trade.
- **The guardrail verifies support exists; it does not verify entailment.** Measured on the
  hosted 27B over 90 recaps: only **1.45%** of its claims had an unlocatable quote, yet it still
  wrote planted invented content (an invented dosage, frequency or diagnosis) into **19.7%** of
  the traps. A model can quote faithfully and still assert detail the quote does not contain.
  Closing that gap needs sentence-level entailment checking, not substring lookup, and that is
  the next engineering step rather than something this version claims.
- The corpus is synthetic. Real validation needs a clinician's own notes — which is the next
  step, and the point of the local design (her data stays hers).
- No EHR integration, no multi-user sync, no billing, no mobile app. Deliberately out of scope.
- Roadmap: clinician-reviewed corpus and a fine-tune (Tinker) on her note format; session-to-
  session carry-forward; a signed audit trail per note.

## Footprint

`docs/FOOTPRINT.md` lists every path this project adds to a machine with its measured size
(4.3 GB removable, 958 MB of it the model), and `scripts/cleanup.sh` removes it in one command.
