# Footprint — what this build adds to a machine, and how to remove it

Confidant is a local-first product, so its honest cost is the model on your disk and the RAM
to hold it. That cost is measured here rather than estimated, on the development laptop
(8 threads, 6.7 GB RAM, no GPU, no sudo), and there is a one-command reversal in
`scripts/cleanup.sh`.

## What we actually installed on the dev machine

| Path | Size | Purpose | Removable |
|---|---|---|---|
| `~/.local/opt/bin/ollama` + `lib/` | ~1.4 GB | user-space Ollama (no sudo on this box) | yes |
| `~/models/` | 958 MB | `gemma3:1b-it-qat` GGUF blob | yes |
| `~/.local/share/confidant/notes.db` | KB | the app's local store (test notes) | yes |
| `~/.config/hf26/devto_api_key` | 24 B | dev.to publish key — **rotate it, don't just delete** | yes |
| `~/hackathon/confidant/target/` | ~1.4 GB | Rust build cache | yes |
| `ui/node_modules/` + `ui/.next/` | ~400 MB | Next.js build toolchain (build-time only) | yes |
| repo sources | ~400 KB | the actual deliverable | no |
| `ui/out/` | ~200 KB | static export the daemon serves | no |

Peak during the build: ~4.5 GB of disk beyond the repo, and the runtime holds ~1.1 GB RSS
while a note is being drafted.

## Measured on this machine (Gemma, CPU only, 8 threads)

| Model | On disk | Load time | Generation |
|---|---|---|---|
| `gemma3:1b-it-qat` | 1.0 GB | ~1 s (warm) | **7.5 tok/s at Ollama's default 3 threads**; thread scaling measured in `eval/bench_threads.py` |
| `gemma3:4b-it-qat` | 4.05 GB (3.2 GB + 0.85 GB vision tower) | **81 s** | **0.26 tok/s** |

The 4B result is the interesting one and it is reported in the submission: with ~3.5 GB of
free RAM, a 4B model swaps and becomes 29× slower than the 1B per token. It is not a
"small laptop" model, whatever the download page implies. The app therefore ships the 1B as
its default, lists whatever the user has installed with sizes, and lets the machine decide
(`/api/models`). A 16 GB or larger machine runs the 4B, and the same code path runs a 12B.

## Clean after submission

```bash
scripts/cleanup.sh              # dry run: lists every path and its size
scripts/cleanup.sh --yes        # stops the runtime, removes models, caches, node_modules
```

Then rotate the dev.to key at https://dev.to/settings/extensions — deleting the file does not
un-publish the key.
