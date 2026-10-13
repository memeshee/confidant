# Confidant: a 1B Gemma drafts a therapist's session notes on her own laptop, and nothing leaves the room

<!-- Draft: repo URL and Render demo URL are filled in. Still to fill before publishing: {{TECH_DOMAIN}} and the eval table (TBD cells). dev.to tags: #devchallenge #hf26challenge -->

It is 9pm, and a solo therapist still owes the day a set of notes. She has two options. Type them — about twenty minutes every day, after the last client. Or pay $69–114 a month for a cloud "AI scribe" that records the session, which her clients did not consent to.

I built the third option. **Confidant** lets her speak a twenty-second recap between clients; a small open-weights Gemma model running on her own laptop drafts the structured note — Subjective, Objective, Assessment, Plan, next focus — and a deterministic guardrail checks every sentence against the words she actually said. A sentence whose words cannot be found in the recap is never filed. It is shown struck through, under "refused by the guardrail".

This is my entry for the Hacktoberfest Open-Source AI Challenge, Week 2: Side Hustle. It runs on a laptop with 8 threads, 6.7 GB of RAM, no GPU, no CUDA. Every number below was measured on that machine, and every claim maps to a file, a command, or a run in the repo.

## What I Built

Confidant is a local-first session-note drafter for one-person practices: therapists, counsellors, physiotherapists, social workers.

The flow is short. She presses record and speaks a messy recap the way she would to a colleague — "Maya came in flat today, said the journaling stopped two weeks ago, we practised the grounding exercise, she'll try it twice before Thursday, and I want to watch the sleep thing next time." Speech-to-text runs locally through faster-whisper. The transcript goes to Gemma 3 1B in Ollama on the same machine, which drafts the note in five fields — the S/O/A/P shape clinicians already write, plus a next-session focus. Each sentence carries the exact transcript span that supports it; hovering a line highlights that span in the side rail. She edits inline, signs, and moves on. The interface is a document, not a dashboard — the note is the product.

Three decisions do most of the work:

- **The note is a view over the transcript.** Every claim stores a verbatim quote from the recap. Grounding is a data-model invariant, not a prompt instruction.
- **Unsupported sentences are refused, not softened.** The guardrail in `daemon/src/validator.rs` normalises a sentence's quote and searches for it in the transcript. If it cannot find it, the sentence never reaches the filed note. The model's failure mode is "this line is missing, and here is what it tried to write" — never a plausible clinical sentence that nobody said.
- **Risk flags are rules, not model output.** A small, auditable rule set over her own words decides the risk chips. The type the note generator returns has no risk field at all: the model is never asked for a risk level.

What it does not do: no diagnosis, no advice, no EHR integration, no multi-user sync, no mobile app, no cloud. The Rust daemon (Axum) binds `127.0.0.1` only, and mutating routes require a run token injected into the page the daemon serves, so a random website cannot post a transcript into the local store. The store is SQLite on her disk. Audio is deleted after transcription — the delete happens before the transcription result is even parsed. Export is Markdown or a print-ready sheet; history stays in the local database.

There is one honest escape hatch: remote mode, opt-in, for hardware that cannot hold a model. It sends the transcript to any OpenAI-compatible endpoint and says so loudly in the UI. It is off by default. In local mode, `pipeline::assert_policy` refuses — a hard error, not a warning — any Ollama base URL that is not loopback, because that single misconfiguration would make the privacy claim false.

## Demo

Live demo: https://confidant-demo.onrender.com

The demo on Render is read-only. It replays real recorded runs of the pipeline on the synthetic corpus, so you can watch the whole flow without installing anything: recap in, drafted note out, and at least one sentence struck through where the guardrail refused it. Underneath the replay is the evaluation table from the next section, rendered from the committed results.

The local demo is the one that matters, and it works with the wifi off:

1. Open the app the daemon serves, and press *Record instead*.
2. Talk for twenty seconds about a session the way you would to a colleague.
3. Stop. The note drafts on the machine; each line shows the span it came from.
4. Find the struck-through line — that is the guardrail refusing something the model invented.
5. Export to print-ready HTML. The footer shows `0.0000 USD/note`, the tokens/sec, and the model name.

The landing page and the docs live at {{TECH_DOMAIN}} <!-- TODO: fill the .tech domain -->.

## Code

Repo: https://github.com/memeshee/confidant

Everything is in there: the Rust daemon (`daemon/src/`), the statically exported Next.js UI (`ui/`), the prompts shared by the product and the eval harness (`prompts/`), the synthetic corpus and the harness (`eval/`), and the footprint documents (`docs/`).

Three commands are worth running:

```bash
cargo test                                   # 15 tests: guardrail, provider policy, exports, store, line parser
python3 eval/run_eval.py --arm rules-only    # the no-model baseline
scripts/cleanup.sh                           # dry run: lists everything this build added to a machine
```

`cargo test` is at 15 passing tests, including one that refuses a fabricated medication dose. `scripts/cleanup.sh` is dry-run verified: 4.3 GB removable, of which 958 MB is the model, and one command reverses everything this build added to a machine.

## How I Built It

The daemon is Rust (Axum + tokio + rusqlite) and serves the statically exported Next.js UI plus the local API on `127.0.0.1`. Inference is Ollama on loopback; transcription is faster-whisper in a sidecar process; the store is SQLite. The prompts are compiled into the daemon and read by the eval harness from the same files, so the published numbers measure the shipped configuration, not a copy of it.

Then the measurements, because "runs on your machine" is a claim until it has a number attached:

- **Gemma 3 1B (QAT, 1.0 GB on disk, Ollama).** Drafted 3 traceable sentences from a 230-word recap in 56 s — about 7.5 tokens/sec on this CPU. The output is thin. It is the shipped default anyway, because it is the one that runs.
- **Gemma 4B (3.2 GB weights + 0.85 GB vision tower).** 81 seconds just to load, then 0.26 tokens/sec. A note would take about four minutes. I deleted it from disk. Whatever the download page implies, it is not a small-laptop model, and I would rather say that than ship it.
- **Gemma 3 27B (open weights, hosted via Nebius Token Factory, opt-in remote mode).** 16 sentences in 9.9 s, all 16 traceable, 0 refused — 549 prompt tokens + 505 completion tokens. That is the trade in full: better notes, faster, and the transcript leaves the machine. Hosted endpoints also do not enforce JSON schemas (I probed), so the remote path uses a flat line protocol parsed in Rust instead of the schema the local path uses.

The evaluation has four arms: a rules-only baseline (deterministic sentence-splitting by trigger words, no model), the local 1B raw, the local 1B behind the guardrail, and the hosted 27B behind the guardrail. The corpus is synthetic and frozen: **90 recaps, 760 annotated facts and 427 planted distractors** — recaps and fact lists written for this project, no real client data anywhere — and the harness is `eval/run_eval.py`; results are committed under `eval/results/`.

<!-- TODO: fill the evaluation table from the final eval run before publishing. -->

| Arm | Fact recall | Facts in right section | Distractor rate | Unsupported claims | Median latency | Cost per note |
|---|---|---|---|---|---|---|
| rules-only baseline (90 recaps) | 99.2% | 40.0% | 11.0% | 0% | ~0 ms | $0 |
| local Gemma 1B raw (8 recaps) | not measured yet | not measured yet | not measured yet | pending | ~190 s under load | $0 |
| local Gemma 1B + guardrail | pending | pending | pending | pending | pending | $0 |
| hosted Gemma 27B + guardrail | pending | pending | pending | pending | pending | provider-priced |

*Distractor rate counts planted, plausible-but-absent claims that the note wrote anyway; unsupported claims are note sentences whose supporting quote cannot be found in the transcript. Every cell here is a real run committed under `eval/results/`, and cells are filled in as the arms complete rather than estimated.*

**The first version of this table was wrong, and the way it was wrong is worth reporting.** My first metric was fact recall: does the note contain the facts. The rules-only baseline scored well over 98% — near-perfect, for a system with no model in it. That was implausible enough that I opened a result, and there it was: the baseline dumps the transcript into every section, so every keyword trivially matched. It was not writing a note; it was copying one. I rewrote the metrics to measure where each fact lands (section placement) and how long the note is relative to the transcript (brevity). The baseline immediately read as what it is: 40% of facts in the right section, and a note exactly as long as the transcript (0.99×) — while the hosted 27B's note is 0.68×, which is what a note looks like. A baseline that looks perfect is usually a measurement bug, not a miracle, and a recall-only metric would have let the no-model arm win the table.

## Why Does Open Innovation Matter?

For this product the answer is structural, not philosophical.

A cloud scribe cannot be this product. Its architecture requires shipping a client's words to somebody else's server — the one thing this product promises never happens. That is a consent problem, not a pricing problem: her clients did not agree to be recorded, and no amount of vendor diligence changes that. The local, open-weights model is the only version of this product that can exist.

Open matters for the rest of it too:

- **She can see what the model was told.** The prompts are files in the repo, not a vendor's secret.
- **She can re-run my numbers.** The harness, the corpus, and the results are in the repo. Nobody has to trust this table.
- **The guardrail is code, not a policy page.** It runs locally, on her machine, and it is unit-tested. Auditable refusal beats promised moderation.
- **$0.0000 per note** is the difference between a tool that exists for a solo practice and one that does not. A per-note meter is a subscription for someone who already pays one.

There is a smaller version of this, too. The model sizes that make the product possible — a 1B that fits in a gigabyte — exist because open weights were released and measured in public. The 4B measurement above (81 seconds to load, 0.26 tokens/sec, deleted) is the kind of thing that only gets said out loud in an open ecosystem; a closed vendor's size chart does not include it.

## My Agent Session

I built this as a series of numbered increments, one at a time, with an agent doing the work in each, under a rule that no increment counts until it ends in a real run. The session log — timestamps, decisions, and what broke — is committed at `docs/CONFIDANT-PLAN.md`.

The rule earned itself. The unit tests were green while the router was broken: axum 0.8 changed path parameters from `:id` to `{id}`, and the daemon panicked when the first real request hit a parameterised route. Unit tests never touch the router, so nothing in the test suite could have caught it. Fixing it meant renaming the path parameters; the daemon has served `/api/health` in local mode since.

The agent also produced one measurement I threw away: a thread-scaling benchmark run while the 4B model was still resident in RAM. Same machine, different load — not valid evidence. The clean re-run is committed at `eval/results/bench-threads-clean.log`. That habit — discard the contaminated number, re-run, commit the artifact — is most of what makes the numbers here worth reading.

The standing rule for the build was never invent evidence. It is why the metric bug above got fixed instead of hidden, and why this post leaves TBD cells instead of confident guesses. The decisions that mattered stayed with me: what ships as the default, and what the product refuses to do. The post itself was drafted with agent assistance, and every number in it was checked against an artifact before it went in.

## My First User

<!-- TODO: clinician's verbatim words, what she asked to change, the price she named -->

No clinician was available within the five-day window, so this section carries no quote and no testimonial; the evidence that someone would actually pay for this is the measured evaluation instead — the baseline that files the transcript as a note, the 1B that produces three traceable sentences and no more, and the 27B comparison that produces sixteen. All of it is re-runnable from the repo with `python3 eval/run_eval.py`. A fabricated quote would have been easy to write, and it would have made every other number in this post suspect.

## Prize Categories

**Best Use of Gemma.** Gemma 3 1B QAT is the shipped default runtime, running on CPU with no GPU: measured at ~7.5 tokens/sec, drafting 3 traceable sentences from a 230-word recap in 56 s. Gemma 3 27B (open weights) is the hosted comparison row — 16 sentences, all 16 traceable, in 9.9 s — and the model picker lists whatever is installed on the machine, with real sizes.

**Best Use of Render.** The public read-only demo of real recorded runs, plus the evaluation table, is hosted on Render: https://confidant-demo.onrender.com (service `srv-db73srid0e5s73avf6i0`, free plan, Docker runtime). Every mutating route answers 403 there — it is a replay, not a live service.

**Best Use of a .tech Domain.** The project's landing page and docs live on a .tech domain claimed through Hacktoberfest: {{TECH_DOMAIN}}.

**Not claimed, and why.** Two categories are described in this post as the measured next step and are explicitly not claimed: Tinker fine-tuning (a LoRA on a clinician's note format — no run yet) and Sentry tracing (stage, latency, token and cost metadata, hashes only, never clinical text — designed, not wired). A claim without a run behind it is exactly what this project refuses to file.

Confidant is a small thing: one therapist, one laptop, one recap, and one note that can prove where every sentence came from. That it can exist at all is a property of open models. That it refuses to write what she did not say is the property I care about most.
