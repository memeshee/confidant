# Confidant — build plan (HF26 Week 2, deadline Oct 18 23:59 PDT = Oct 19 13:59 ICT)

Approved memo: `~/hackathon/hf26-week2-memo.md`. This file is the increment list; append to the
progress log at the bottom as increments land. One increment at a time, each ends in a real run.

## Fact base (verified, do not re-litigate)
- Field: ~1,000–2,000 posts per challenge. My scan of all 386 posts published Oct 12+:
  Gemma 147 claims · Render 49 · ElevenLabs 25 · Entire 22 · TabPFN 19 · SerpApi 17 · Copilot 16 ·
  MongoDB 15 · Sentry 12 · Mastra 9 · Backboard 7 · **Tinker 5** · Temporal 5 · Arduino 5 ·
  **DigitalOcean 4** · Tiger Data 4 · **.tech 1**.
- Winner traits (17/17 weekend winners): named beneficiary + verbatim words; "model suggests, code
  decides"; quantified eval with baselines and a held-out set; a named failure and its fix;
  local/privacy load-bearing; repo + demo link; template write-up ~1,800–2,900 words.
  Median winner had 5 reactions — the panel, not the crowd, decides.
- Machine: 8 threads, 6.7 GB RAM (≈3.9 GB available), no CUDA, no cmake, **no sudo**.
  Gemma 4 E2B is 4.6 GB min → does not fit. Local default must be Gemma 3 1B/4B QAT.
- Ollama installs user-space from the release tarball (no sudo). DEV key: @memeshe, id 2192841,
  stored 0600 at `~/.config/hf26/devto_api_key`. Draft post id in `~/.config/hf26/devto_draft_id`.
- Publish path verified: draft 4844582 created via API (`published:false`).

## Increments (dependency order)

- [ ] **I1 — runtime up.** Ollama user-space install; pull `gemma3:1b-it-qat`; measure tok/s
      CPU-only. Evidence: `ollama --version`, `ollama ps`, a timed generation.
- [ ] **I2 — repo skeleton.** `~/hackathon/confidant`: Rust daemon (Axum) + `crates/`, Next.js
      static UI, `docs/`. Evidence: `cargo build` + `npm run build` both green.
- [ ] **I3 — pipeline end to end (Gate 1).** transcript text in → schema-constrained note JSON
      out, validated by the deterministic guardrail. Evidence: a real run on a scripted recap.
- [ ] **I4 — local audio.** faster-whisper (venv via uv) transcription + audio deletion path.
      Evidence: a real .wav → transcript, audio file gone afterwards.
- [ ] **I5 — UI document surface.** One screen: recap → drafted note with per-line provenance,
      edit inline, export MD/PDF. Evidence: screenshot + a run in the browser.
- [ ] **I6 — corpus + eval harness.** ≥120 held-out recaps; 4 arms (rules-only, base small,
      tuned small, one large hosted); per-field F1, unsupported-claim rate, p50 latency, $/note.
      Evidence: `eval/results.json` + the rendered table.
- [ ] **I7 — Tinker fine-tune.** Qwen3.5-4B LoRA on the note-structuring task; baseline→tuned
      delta on held-out. Fallback if access stalls: report base vs large only (drop the category).
- [ ] **I8 — observability.** Sentry tracing (stage, latency, tokens, cost, hashes only) +
      the debugging story the traces produced.
- [ ] **I9 — public surfaces.** Render demo (scripted synthetic replay + eval dashboard) and the
      `.tech` landing/docs; README; architecture diagram; favicon from the design palette.
- [ ] **I10 — first user.** Therapist session(s), her verbatim words, what she asked to change,
      the price she named. No quote is ever invented; skip the section if it does not happen.
- [ ] **I11 — submission.** dev.to post (template H2s, tags `devchallenge, hf26challenge`),
      every number re-verified against artifacts, published ≥6h before the deadline.

## Kill gates
1. **Gate 1 (Oct 14 EOD)** — spoken recap → drafted note end to end on this laptop. If latency
   or quality fails: drop to text-in only (voice becomes the demo's stretch) — the note
   pipeline stays.
2. **Gate 2 (Oct 16 EOD)** — eval table with ≥120 held-out items + one engineered-out failure
   documented, or the post leads with a smaller honest table and says so.
3. **Gate 3 (Oct 17 EOD)** — post drafted with per-category evidence; publish Oct 18 morning ICT.

## Standing rules for this build
- **Leave the device clean.** Every install goes in the footprint table (`docs/FOOTPRINT.md`)
  and must be removable with `scripts/cleanup.sh`. The dev box is 6.7 GB RAM / 8 threads: no
  model bigger than what fits, no parallel heavy jobs, and no "just in case" dependencies.
- Never invent evidence. Synthetic corpus is labelled synthetic in the post.
- Discard contaminated measurements: the first thread-scaling run happened while the 4B was
  still resident, so it was thrown away and re-run clean.
- One accent colour, one display face, one mono face; no cards/shadows/gradients; the note is
  the interface.
- Secrets live in `~/.config/hf26/` (0600) and never enter the repo. Run the secret-hygiene
  check before any push. Rotate the dev.to key after publication.
- Claims in the post must map to an artifact or a number produced by a real run.

## Decisions log
- 2026-10-13 21:35 ICT — **4B model dropped.** `gemma3:4b-it-qat` measured 81 s load and
  **0.26 tok/s** (swap-thrashing in ~3.5 GB free RAM) versus 7.5 tok/s for the 1B. Deleted
  from disk (freed 4 GB); shipped default is the 1B, with the model picker and the measured
  table carrying the "your machine decides" story.
- 2026-10-13 — Runtime is user-space Ollama (no sudo on this box); models in `~/models`.

## Progress log
- 2026-10-13 20:10 ICT — memo approved; runtime download started; publish path verified
  (draft 4844582); repo + `.gitignore` (secrets first) created.
- 2026-10-13 20:45 ICT — Rust daemon compiles; **11/11 tests green**; validator locates real
  spans and refuses invented quotes (unit-tested).
- 2026-10-13 21:12 ICT — **Gate 1 met**: real end-to-end run on `eval/fixtures/recap-01.txt`
  → 3/3 claims traced to real transcript spans, rule-derived `denies_si` flag, $0.0000/note.
- 2026-10-13 21:36 ICT — 4B measured and removed; footprint + cleanup script written and
  dry-run verified (4.3 GB removable).
- 2026-10-13 21:50 ICT — **remote mode added and working.** Nebius Token Factory reaches
  `google/gemma-3-27b-it`; same fixture → **16/16 claims traceable, 0 refused, 9.9 s** vs the
  local 1B's 3 claims in 56 s. Hosted endpoints do not enforce JSON schemas (measured with a
  tiny probe), so the remote path uses a flat line protocol (`prompts/note_remote.txt`) and
  `parse_lines`, unit-tested. 15/15 tests green.
- 2026-10-13 22:05 ICT — runtime bug found by a real run: axum 0.8 needs `{id}` not `:id`
  (**the router panicked at startup**; unit tests cannot catch it). Fixed; daemon serves on
  8791 with `/api/health` green in local mode.
- 2026-10-13 22:10 ICT — **metric bug found by an implausible result**: the no-model baseline
  scored perfect fact recall because it dumps the transcript into sections. Replaced with
  section-assignment accuracy + brevity. Baseline now reads 44% right-section, 1.98× the
  transcript length — i.e. it is not writing a note, and the metric says so.
- 2026-10-13 22:15 ICT — corpus: 80 recaps / 691 facts / 381 planted distractors (part-01
  pending, target 110). README written with a real/measured/synthetic table.

### Measurement hygiene note
Local-arm latency measured while `npm install`/builds run in parallel is **not valid evidence**
for the post: same machine, different load. Accuracy is unaffected (deterministic scoring), so
accuracy comes from these runs; any latency claim gets re-measured on a quiet machine or is
reported as "under load" with the range.

## RESUME POINT (state as of 2026-10-13 21:15 ICT)

**Done and verified:** Rust daemon (15/15 tests) · guardrail with committed refusal evidence
(`docs/evidence/refusal-example.json`) · local + opt-in remote inference · model picker ·
local STT verified with a real transcript and audio deletion observed · UI built and served by
the daemon (fonts self-hosted, favicon set) · README with a real/measured/synthetic table ·
footprint + `scripts/cleanup.sh` (dry-run verified) · corpus frozen (90 recaps, 760 facts,
427 distractors) · post draft at `docs/devto-post-draft.md` (2,400 words, reviewed, numbers
corrected after the metric change).

**Measured so far (all committed under `eval/results/`):**

| arm | n | recall | right-section | distractors | unsupported | brevity | median |
|---|---|---|---|---|---|---|---|
| rules-only | 90 | 99.2% | 40.0% | 11.0% | 0% | 0.99 | ~0 ms |
| local 1B raw | 5 of 8 done | in progress | in progress | 0% | 67–80% on early recaps | 0.47–0.76 | 98–322 s (loaded) |
| local 1B + guardrail | not started | | | | | | |
| hosted 27B raw | 90 | 72.8% | 45.9% | 19.7% | 1.45% | 0.68 | 5.1 s |
| hosted 27B + guardrail | 2 of 8 done | 9/9, 8/9 | 4/9, 6/9 | 0/4 | 0/14, 0/10 | 0.94, 0.68 | ~20 s |

**What remains, in order:**
1. Finish the two running arms (local 1B raw: `eval/results/local-raw-8.log`; hosted 27B +
   guardrail: `eval/results/remote-guard-8.log`), then run local 1B + guardrail via the daemon
   on 8791 (local mode, token `eval-token`). Commands are in "Final measurement protocol".
2. Fill the table in `docs/devto-post-draft.md` with those numbers; any cell without a run
   stays "not measured".
3. Fill `{{GITHUB_REPO_URL}}`, `{{RENDER_DEMO_URL}}`, `{{TECH_DOMAIN}}` (the Render child
   returns the first two; the third needs the user to claim the domain).
4. Publish to dev.to via the API (draft id in `~/.config/hf26/devto_draft_id`, key in
   `~/.config/hf26/devto_api_key`), tags `devchallenge, hf26challenge`, before Oct 18 23:59 PDT.
5. `scripts/cleanup.sh --yes`, then tell the user to rotate all three keys.


Each row of the post's eval table comes from one of these. The corpus is frozen at 90 recaps /
760 facts / 427 planted distractors (`eval/corpus/part-0{2,3,4}.json`).

```bash
# 1. baseline, no model, whole corpus (instant)
python3 eval/run_eval.py --arm rules-only

# 2. local 1B raw (no guardrail) -- this is the hallucination measurement
python3 eval/run_eval.py --arm local --model gemma3:1b-it-qat --limit 8

# 3. local 1B through the SHIPPED daemon (guardrail applied)
CONFIDANT_TOKEN=eval-token CONFIDANT_PORT=8791 \
  python3 eval/run_eval.py --arm local+guard --model gemma3:1b-it-qat --limit 8

# 4. hosted Gemma 27B raw + its guardrail row: run a second daemon in remote mode on 8795
CONFIDANT_PROVIDER=remote CONFIDANT_REMOTE_KEY=$(cat ~/.config/hf26/tokenfactory_key) \
CONFIDANT_MODEL=google/gemma-3-27b-it CONFIDANT_PORT=8795 CONFIDANT_TOKEN=eval-token \
  ./target/debug/confidant-daemon &
CONFIDANT_TOKEN=eval-token CONFIDANT_PORT=8795 \
  python3 eval/run_eval.py --arm local+guard --model google/gemma-3-27b-it --limit 12
```

Rows to publish: rules-only · local-1B raw · local-1B + guardrail · hosted-27B + guardrail.
Columns: fact recall · facts in the right section · distractor rate · unsupported claims ·
refused by guardrail · median ms · USD/note.

**Rule for the post:** any cell without a real run behind it is written as `not measured`, never
as a plausible-looking number.



