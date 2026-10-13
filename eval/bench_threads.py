#!/usr/bin/env python3
"""Local inference benchmark for Confidant. CPU only, no network beyond loopback.

Reports tokens/sec per (model, thread count) on a fixed prompt, plus peak RSS of the
runtime, so the shipped default is a measurement rather than a guess.
"""
import json
import subprocess
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:11434"
PROMPT = (
    "Summarise in three sentences: a client reports improved sleep after starting a wind-down "
    "routine, ongoing work stress from a restructure, and grief about an estranged sister."
)


def post(path: str, payload: dict, timeout: int = 900) -> dict:
    req = urllib.request.Request(
        BASE + path, data=json.dumps(payload).encode(), headers={"content-type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def unload(model: str) -> None:
    try:
        post("/api/generate", {"model": model, "keep_alive": 0, "prompt": ""}, timeout=60)
    except Exception:
        pass


def bench(model: str, threads: int, predict: int = 96) -> dict:
    unload(model)
    time.sleep(1)
    t0 = time.time()
    try:
        out = post(
            "/api/generate",
            {
                "model": model,
                "prompt": PROMPT,
                "stream": False,
                "keep_alive": "2m",
                "options": {"num_predict": predict, "num_thread": threads, "temperature": 0.2, "num_ctx": 4096},
            },
        )
    except Exception as e:  # model too large for this machine, or runtime died
        return {"model": model, "threads": threads, "error": str(e)[:200]}
    wall = time.time() - t0
    ev_ns = out.get("eval_duration", 0) or 1
    return {
        "model": model,
        "threads": threads,
        "load_ms": round((out.get("load_duration", 0) or 0) / 1e6),
        "eval_count": out.get("eval_count"),
        "tok_s": round((out.get("eval_count", 0)) / (ev_ns / 1e9), 2),
        "wall_s": round(wall, 1),
    }


def rss_mb() -> int:
    try:
        out = subprocess.run(["ps", "-eo", "rss,comm"], capture_output=True, text=True).stdout
        return sum(int(l.split()[0]) for l in out.splitlines() if "llama-server" in l) // 1024
    except Exception:
        return -1


def main() -> int:
    models = sys.argv[1:] or ["gemma3:1b-it-qat"]
    rows = []
    for m in models:
        for th in (2, 4, 6, 8):
            r = bench(m, th)
            r["llama_server_rss_mb"] = rss_mb()
            rows.append(r)
            print(json.dumps(r), flush=True)
    print(json.dumps({"rows": rows}), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
