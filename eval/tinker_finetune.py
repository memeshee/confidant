"""Fine-tune the note-structuring task on Tinker, for real, and score the result.

What this is: a first LoRA run on `Qwen/Qwen3.5-4B` over the note-structuring task the eval
measures — input is a recap transcript, output is the note JSON the daemon's schema expects.

What this is not: the clinician-corpus fine-tune the post describes as the next step. The
teacher here is the deterministic `rules_only` floor from `eval/run_eval.py`, so the model is
learning the task's *shape* from a floor we can measure against, not a clinician's judgement.
Saying "wired, run, here are the numbers — and here is what it does not yet claim" is the point.

    TINKER_API_KEY=... python3 eval/tinker_finetune.py --steps 40 --holdout 6

Writes eval/results/tinker-lora-<stamp>.json.
"""
from __future__ import annotations

import argparse
import glob
import importlib.util
import json
import os
import random
import re
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SECTIONS = ["subjective", "objective", "assessment", "plan", "next_focus"]
SYSTEM = (ROOT / "prompts" / "note_system.txt").read_text().strip()

# reuse the eval's own scorer and its deterministic floor, so the numbers mean the same thing
spec = importlib.util.spec_from_file_location("run_eval", ROOT / "eval" / "run_eval.py")
run_eval = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run_eval)


def load_corpus(limit: int | None = None) -> list[dict]:
    recaps: list[dict] = []
    for path in sorted(glob.glob(str(ROOT / "eval" / "corpus" / "part-*.json"))):
        recaps.extend(json.loads(Path(path).read_text())["recaps"])
    return recaps[:limit] if limit else recaps


def parse_note_json(text: str) -> dict:
    """The model was told: no fence, no preamble. Take it at its word, with one rescue attempt."""
    text = text.strip()
    try:
        v = json.loads(text)
    except Exception:
        m = re.search(r"\{.*\}", text, re.S)
        v = json.loads(m.group(0)) if m else {}
    return {s: [c for c in (v.get(s) or []) if isinstance(c, dict)] for s in SECTIONS}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="Qwen/Qwen3.5-4B")
    ap.add_argument("--rank", type=int, default=16)
    ap.add_argument("--steps", type=int, default=40)
    ap.add_argument("--batch", type=int, default=2)
    ap.add_argument("--lr", type=float, default=1e-4)
    ap.add_argument("--holdout", type=int, default=6)
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--sample-tokens", type=int, default=900)
    args = ap.parse_args()

    if not os.environ.get("TINKER_API_KEY"):
        print("TINKER_API_KEY is not set", file=sys.stderr)
        return 2

    import tinker
    from tinker import types

    recaps = load_corpus(args.limit)
    holdout = recaps[-args.holdout:]
    train = recaps[: -args.holdout] if args.holdout else recaps
    print(f"corpus {len(recaps)} · train {len(train)} · holdout {len(holdout)} ({[r['id'] for r in holdout]})")

    sc = tinker.ServiceClient()
    tc = sc.create_lora_training_client(base_model=args.model, rank=args.rank,
                                        user_metadata={"project": "confidant", "task": "note-structuring"})
    tok = tc.get_tokenizer()

    def chat_tokens(messages: list[dict], generation: bool) -> list[int]:
        # Render first, then tokenise explicitly: `tokenize=True` on a chat template returns a
        # dict under transformers 5.x, and the flat list of ids is the only shape the trainer
        # can use. (First run of this script died on exactly that: every "token list" had len 2.)
        try:
            text = tok.apply_chat_template(messages, tokenize=False,
                                           add_generation_prompt=generation, enable_thinking=False)
        except TypeError:                      # template without the knob
            text = tok.apply_chat_template(messages, tokenize=False, add_generation_prompt=generation)
        ids = tok(text)["input_ids"]
        if len(ids) < 16:
            raise RuntimeError(f"tokenising produced {len(ids)} ids for a chat template — not a real sequence")
        return ids

    # ---------- dataset: transcript -> note JSON, teacher = the deterministic floor ----------
    examples = []
    for r in train:
        target = json.dumps(run_eval.rules_only(r["transcript"]))
        prompt_ids = chat_tokens(
            [{"role": "system", "content": SYSTEM},
             {"role": "user", "content": f"RECAP (verbatim, may be messy):\n\n{r['transcript']}"}],
            generation=False)
        full_ids = chat_tokens(
            [{"role": "system", "content": SYSTEM},
             {"role": "user", "content": f"RECAP (verbatim, may be messy):\n\n{r['transcript']}"},
             {"role": "assistant", "content": target}],
            generation=False)
        examples.append((prompt_ids, full_ids, r["id"]))
    print("example token lengths:", [len(f) for _, f, _ in examples][:5], "...")

    def datum(prompt_ids: list[int], full_ids: list[int]):
        x, y, w = full_ids[:-1], full_ids[1:], [0.0] * (len(full_ids) - 1)
        for i in range(len(prompt_ids) - 1, len(w)):     # supervise only the assistant turn
            w[i] = 1.0
        return types.Datum(
            model_input=types.ModelInput.from_ints(tokens=x),
            loss_fn_inputs={"target_tokens": types.TensorData(data=y, dtype="int64"),
                            "weights": types.TensorData(data=w, dtype="float32")})

    # ---------- train ----------
    rng = random.Random(0)
    losses, t0 = [], time.time()
    for step in range(1, args.steps + 1):
        picked = rng.sample(examples, args.batch) if len(examples) > args.batch else examples
        batch = [datum(p, f) for p, f, _ in picked]
        fb = tc.forward_backward(batch, loss_fn="cross_entropy").result()
        opt = tc.optim_step(adam_params=types.AdamParams(learning_rate=args.lr)).result()
        m = getattr(fb, "metrics", None)
        if step == 1:
            print("  [debug] metrics:", json.dumps(m, default=str)[:300] if m is not None else None)
        loss = None
        if isinstance(m, dict):
            for k in ("loss", "mean_loss", "loss:mean", "cross_entropy"):
                if k in m:
                    loss = float(m[k])
                    break
            if loss is None and "loss:sum" in m:
                # Divide by the tokens the loss actually sees: the prompt's are masked to 0,
                # so using the full sequence length would report a ~2.5x smaller number.
                n = sum(max(len(f) - len(p), 1) for p, f, _ in picked)
                loss = float(m["loss:sum"]) / max(n, 1)
        if loss is None:
            outs = getattr(fb, "loss_fn_outputs", None) or []
            vals = []
            for o in outs:
                el = (o.get("elementwise_loss") if isinstance(o, dict) else None)
                if el is not None and getattr(el, "data", None):
                    vals.extend(el.data)
            if vals:
                loss = sum(vals) / len(vals)
        losses.append(loss)
        if step % 5 == 1 or step == args.steps:
            shown = f"{loss:.4f}" if isinstance(loss, float) else "n/a"
            print(f"  step {step:>3}/{args.steps}  loss {shown}  ({time.time()-t0:.0f}s)")

    # ---------- sample the held-out recaps ----------
    samp = tc.save_weights_and_get_sampling_client(name=f"confidant-{int(time.time())}")
    rows, scored = [], []
    for r in holdout:
        ids = chat_tokens(
            [{"role": "system", "content": SYSTEM},
             {"role": "user", "content": f"RECAP (verbatim, may be messy):\n\n{r['transcript']}"}],
            generation=True)
        resp = samp.sample(prompt=types.ModelInput.from_ints(tokens=ids), num_samples=1,
                           sampling_params=types.SamplingParams(max_tokens=args.sample_tokens,
                                                                temperature=0.2, top_p=0.95)).result()
        seq = resp.sequences[0]
        text = tok.decode(list(seq.tokens), skip_special_tokens=True)
        draft = parse_note_json(text)
        sc_ = run_eval.score(r, draft, r["transcript"])
        base = run_eval.score(r, run_eval.rules_only(r["transcript"]), r["transcript"])
        scored.append((sc_, base))
        rows.append({"id": r["id"], "valid_json": bool(draft and any(draft.values())),
                     "tuned": sc_, "rules_only_floor": base, "sample": text[:4000]})
        print(f"  {r['id']}  tuned recall {sc_['facts_hit']}/{sc_['facts']} right-section "
              f"{sc_['facts_in_right_section']}/{sc_['facts']} · floor {base['facts_hit']}/{base['facts']}")

    def mean(rs, k):
        return round(sum(x[k] for x in rs) / max(len(rs), 1), 3)

    tuned_mean = {k: mean([a for a, _ in scored], k) for k in
                  ["facts_hit", "facts_in_right_section", "distractors_hit", "claims", "claims_unsupported", "note_words", "brevity"]}
    floor_mean = {k: mean([b for _, b in scored], k) for k in tuned_mean}
    stamp = time.strftime("%Y%m%d-%H%M%S")
    out = ROOT / "eval" / "results" / f"tinker-lora-{stamp}.json"
    out.write_text(json.dumps({
        "config": {"base_model": args.model, "rank": args.rank, "steps": args.steps,
                   "batch": args.batch, "lr": args.lr, "holdout": [r["id"] for r in holdout],
                   "teacher": "rules_only (eval/run_eval.py) — the deterministic floor, not a clinician",
                   "trained_ids": [i for _, _, i in examples]},
        "losses": losses,
        "summary": {"tuned": tuned_mean, "rules_only_floor_on_the_same_heldout": floor_mean,
                    "note": "the tuned arm is compared with the same scorer the committed eval uses"},
        "rows": rows,
    }, indent=2))
    print("tuned  ", tuned_mean)
    print("floor  ", floor_mean)
    print("wrote", out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
