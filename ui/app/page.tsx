"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

type Span = { start: number; end: number };
type Claim = { text: string; quote: string; span?: Span; span_text?: string };
type Draft = {
  subjective: Claim[];
  objective: Claim[];
  assessment: Claim[];
  plan: Claim[];
  next_focus: Claim[];
};
type RiskFlag = { kind: string; source: string; evidence: string };
type Stats = {
  claims_total: number;
  claims_verified: number;
  claims_unverified: number;
  gen_ms: number;
  verify_ms: number;
  stt_ms: number;
  prompt_tokens: number;
  output_tokens: number;
  tokens_per_sec: number;
  cost_usd: number;
};
type Note = {
  id: string;
  created_at: string;
  model: string;
  transcript: string;
  draft: Draft;
  risk_flags: RiskFlag[];
  unverified: Claim[];
  stats: Stats;
};
type Brief = {
  id: string;
  created_at: string;
  model: string;
  verified: number;
  refused: number;
  first_line: string;
};
type Health = {
  ok: boolean;
  runtime: { model?: string; installed?: string[]; error?: string; outbound_network?: boolean };
  transcriber: { available: boolean; model: string };
};
type EvalPayload = { summary?: Record<string, unknown>; rows?: unknown[] };
/** What `GET /api/demo` returns on the public deployment: recorded runs, no live service. */
type DemoPayload = {
  note: Note;
  note_b?: Note | null;
  eval?: EvalPayload | null;
  eval_file?: string | null;
  mode: string;
  read_only: boolean;
  provenance?: string;
};

const SECTIONS: [keyof Draft, string][] = [
  ["subjective", "Subjective"],
  ["objective", "Objective"],
  ["assessment", "Assessment"],
  ["plan", "Plan"],
  ["next_focus", "Next session focus"],
];

const FLAG_COPY: Record<string, string> = {
  self_harm: "self-harm / suicidal ideation",
  harm_to_other: "risk to others",
  substances: "substances",
  safeguarding: "safeguarding",
  none_stated: "no risk phrase stated",
};

function runToken(): string {
  if (typeof document === "undefined") return "";
  return document.querySelector('meta[name="confidant-token"]')?.getAttribute("content") ?? "";
}

async function jsonApi<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: { "content-type": "application/json", "x-confidant-token": runToken(), ...(init?.headers ?? {}) },
  });
  const raw = await res.text();
  let data: unknown = {};
  try {
    data = JSON.parse(raw);
  } catch {
    data = { error: raw.slice(0, 300) };
  }
  if (!res.ok) throw new Error((data as { error?: string })?.error ?? res.statusText);
  return data as T;
}

function stamp(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString(undefined, { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" });
}

function Rail({ transcript, active }: { transcript: string; active: string | null }) {
  const ref = useRef<HTMLDivElement>(null);
  const parts = useMemo(() => {
    if (!active) return [transcript];
    const i = transcript.indexOf(active);
    if (i < 0) return [transcript];
    return [transcript.slice(0, i), active, transcript.slice(i + active.length)];
  }, [transcript, active]);

  useEffect(() => {
    const mark = ref.current?.querySelector("mark.source");
    if (mark) mark.scrollIntoView({ block: "center", behavior: "auto" });
  }, [active]);

  return (
    <div ref={ref} className="max-h-[62vh] overflow-y-auto pr-1">
      <p className="whitespace-pre-wrap text-[15px] leading-relaxed">
        {parts.length === 3 ? (
          <>
            {parts[0]}
            <mark className="source">{parts[1]}</mark>
            {parts[2]}
          </>
        ) : (
          parts[0]
        )}
      </p>
    </div>
  );
}

function ClaimLine({
  claim,
  section,
  index,
  noteId,
  onActive,
  onSaved,
  readOnly = false,
}: {
  claim: Claim;
  section: keyof Draft;
  index: number;
  noteId: string;
  onActive: (span: string | null) => void;
  onSaved: (n: Note) => void;
  readOnly?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(claim.text);

  if (editing) {
    return (
      <li className="mb-3">
        <textarea
          className="border-b border-[color:var(--color-rule)] py-1 text-[17px]"
          rows={2}
          value={value}
          autoFocus
          onChange={(e) => setValue(e.target.value)}
        />
        <div className="mt-1 flex gap-2">
          <button
            className="quiet"
            onClick={async () => {
              const note = await jsonApi<Note>(`/api/notes/${noteId}`, {
                method: "PATCH",
                body: JSON.stringify({ section, index, text: value }),
              });
              setEditing(false);
              onSaved(note);
            }}
          >
            Save
          </button>
          <button className="quiet" onClick={() => { setValue(claim.text); setEditing(false); }}>
            Cancel
          </button>
          <span className="io self-center">your edit is kept as written; it is not checked against the recap</span>
        </div>
      </li>
    );
  }

  return (
    <li
      className="claim mb-2 cursor-default px-1 py-0.5"
      data-active={claim.span_text ? undefined : "false"}
      onMouseEnter={() => onActive(claim.span_text ?? null)}
      onMouseLeave={() => onActive(null)}
    >
      <span>{claim.text}</span>
      {!readOnly && (
        <button
          className="io ml-2 underline decoration-dotted opacity-0 transition-opacity hover:opacity-100 focus:opacity-100"
          style={{ opacity: undefined }}
          onClick={() => setEditing(true)}
          aria-label={`Edit: ${claim.text.slice(0, 40)}`}
        >
          edit
        </button>
      )}
      {claim.span_text ? (
        <span className="io block pl-3">↳ “{claim.span_text}”</span>
      ) : null}
    </li>
  );
}

function NoteView({ note, onSaved, readOnly = false }: { note: Note; onSaved: (n: Note) => void; readOnly?: boolean }) {
  const [active, setActive] = useState<string | null>(null);
  const s = note.stats;
  return (
    <article className="grid gap-10 lg:grid-cols-[1.55fr_1fr]">
      <div>
        <header className="flex flex-wrap items-baseline justify-between gap-3 border-b border-[color:var(--color-rule)] pb-4">
          <div>
            <div className="label">session note</div>
            <h1 className="font-[family-name:var(--font-display)] text-[26px] leading-tight tracking-tight">
              {stamp(note.created_at)}
            </h1>
          </div>
          <div className="flex items-center gap-2">
            <a className="quiet" href={`/api/notes/${note.id}/export?format=md`}>Markdown</a>
            <a className="quiet" href={`/api/notes/${note.id}/export?format=html`} target="_blank" rel="noreferrer">
              Print / PDF
            </a>
          </div>
        </header>

        <div className="mt-5 flex flex-wrap gap-2">
          {note.risk_flags.map((f, i) => (
            <span
              key={`${f.source}-${i}`}
              className={
                f.kind === "none_stated"
                  ? "io rounded-full border border-[color:var(--color-rule)] px-3 py-1"
                  : "io rounded-full border border-[color:var(--color-accent)] px-3 py-1 text-[color:var(--color-accent)]"
              }
              title={`${f.source}${f.evidence ? ` · matched “${f.evidence}”` : ""}`}
            >
              {FLAG_COPY[f.kind] ?? f.kind}
            </span>
          ))}
          <span className="io self-center">risk flags are rule-derived from your words — the model never writes them</span>
        </div>

        {SECTIONS.map(([key, title]) => (
          <section key={key}>
            <h2 className="label mt-9 border-t border-[color:var(--color-rule)] pt-4">{title}</h2>
            {note.draft[key].length === 0 ? (
              <p className="text-[color:var(--color-muted)] italic">Not stated in the recap.</p>
            ) : (
              <ul>
                {note.draft[key].map((c, i) => (
                  <ClaimLine
                    key={`${key}-${i}`}
                    claim={c}
                    section={key}
                    index={i}
                    noteId={note.id}
                    onActive={setActive}
                    onSaved={onSaved}
                    readOnly={readOnly}
                  />
                ))}
              </ul>
            )}
          </section>
        ))}

        {note.unverified.length > 0 && (
          <section className="mt-9 border-t border-[color:var(--color-rule)] pt-4">
            <h2 className="label text-[color:var(--color-accent)]">refused by the guardrail</h2>
            <p className="io mb-3">
              the model wrote these and code could not find the words in your recap, so they were not filed
            </p>
            <ul>
              {note.unverified.map((c, i) => (
                <li key={i} className="refused mb-1">
                  {c.text}
                </li>
              ))}
            </ul>
          </section>
        )}

        <footer className="io mt-10 flex flex-wrap gap-x-4 gap-y-1 border-t border-[color:var(--color-rule)] pt-4">
          <span>
            {s.claims_verified}/{s.claims_total} sentences traced
          </span>
          <span>{s.claims_unverified} refused</span>
          {s.stt_ms > 0 && <span>transcribe {s.stt_ms} ms</span>}
          <span>draft {s.gen_ms} ms</span>
          <span>guardrail {s.verify_ms} ms</span>
          <span>{s.tokens_per_sec.toFixed(1)} tok/s</span>
          <span>${s.cost_usd.toFixed(4)}/note</span>
          <span>{note.model}</span>
        </footer>
      </div>

      <aside className="lg:border-l lg:border-[color:var(--color-rule)] lg:pl-8">
        <h2 className="label">what you actually said</h2>
        <p className="io mb-3">hover a sentence on the left to see its source</p>
        <Rail transcript={note.transcript} active={active} />
      </aside>
    </article>
  );
}

/** The evaluation table, straight from the newest `eval/results/*.json` — no editing, no paraphrase. */
function EvalTable({ eval: e, file }: { eval: EvalPayload; file?: string | null }) {
  const s = e.summary ?? {};
  const rows = e.rows ?? [];
  const fmt = (v: unknown) =>
    typeof v === "number" ? (Number.isInteger(v) ? String(v) : v.toFixed(4)) : String(v);
  return (
    <section className="mt-12">
      <h2 className="label border-t border-[color:var(--color-rule)] pt-4">
        evaluation · {rows.length} recorded recaps
      </h2>
      <p className="io mb-3">
        produced by <code>eval/run_eval.py</code> — the harness and the guardrail it measures are the ones in this
        repo{file ? ` · ${file}` : ""}. Fixture recaps carry planted distractors; unsupported-rate counts sentences
        the guardrail refused.
      </p>
      <table className="w-full max-w-3xl border-collapse text-[14px]">
        <tbody>
          {Object.entries(s).map(([k, v]) => (
            <tr key={k} className="border-b border-[color:var(--color-rule)]">
              <td className="py-1 pr-6 text-[color:var(--color-muted)]">{k.replace(/_/g, " ")}</td>
              <td className="py-1 tabular-nums">{fmt(v)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

/** The public deployment: a replay of real recorded runs plus the eval table. Input is not accepted. */
function DemoView({ demo }: { demo: DemoPayload }) {
  const runs = useMemo(
    () =>
      [
        { id: "a" as const, label: "run 1 · fixture recap", note: demo.note },
        ...(demo.note_b ? [{ id: "b" as const, label: "run 2 · synthetic recap", note: demo.note_b }] : []),
      ],
    [demo]
  );
  const [id, setId] = useState<"a" | "b">("a");
  const note = runs.find((r) => r.id === id)?.note ?? demo.note;

  return (
    <>
      <section className="border-l-2 border-[color:var(--color-accent)] pl-4">
        <div className="label text-[color:var(--color-accent)]">recorded replay · read-only demo</div>
        <p className="mt-1 max-w-[84ch] text-[15px] leading-relaxed">
          This page is a replay, not a service. The note{demo.note_b ? "s" : ""} below came from real runs of this
          daemon against a local model on the author&apos;s laptop, and the table at the bottom is the repo&apos;s
          own evaluation output. No input is accepted here: the daemon answers every POST, PATCH and DELETE with{" "}
          <code>403</code> before it reaches a handler — nothing a visitor types could ever be stored, drafted or
          sent anywhere.
        </p>
        {demo.provenance && <p className="io mt-2">{demo.provenance}</p>}
      </section>

      <div className="mt-8 flex flex-wrap items-center gap-2">
        <span className="label mr-2">the recorded runs</span>
        {runs.map((r) => (
          <button key={r.id} className={r.id === id ? "primary" : "quiet"} onClick={() => setId(r.id)}>
            {r.label}
          </button>
        ))}
      </div>

      <div className="mt-8">
        <NoteView note={note} onSaved={() => {}} readOnly />
      </div>

      {demo.eval && <EvalTable eval={demo.eval} file={demo.eval_file} />}
    </>
  );
}

export default function Page() {
  const [mode, setMode] = useState<"new" | "note">("new");
  const [notes, setNotes] = useState<Brief[]>([]);
  const [note, setNote] = useState<Note | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [health, setHealth] = useState<Health | null>(null);
  const [demo, setDemo] = useState<DemoPayload | null>(null);
  const [recording, setRecording] = useState(false);
  const recorder = useRef<MediaRecorder | null>(null);

  const refresh = useCallback(async () => {
    try {
      const r = await jsonApi<{ notes: Brief[] }>("/api/notes");
      setNotes(r.notes);
    } catch {
      /* the list is not worth an error banner */
    }
  }, []);

  useEffect(() => {
    refresh();
    jsonApi<Health>("/api/health").then(setHealth).catch(() => setHealth(null));
    // Present on the public deployment only; a local daemon answers 404 and this stays null.
    fetch("/api/demo")
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setDemo(d))
      .catch(() => setDemo(null));
  }, [refresh]);

  useEffect(() => {
    if (!busy) return;
    const t0 = Date.now();
    const id = setInterval(() => setElapsed(Date.now() - t0), 200);
    return () => clearInterval(id);
  }, [busy]);

  async function draft(body: BodyInit, isForm = false) {
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(isForm ? "/api/recap/audio" : "/api/recap/text", {
        method: "POST",
        headers: isForm ? { "x-confidant-token": runToken() } : { "content-type": "application/json", "x-confidant-token": runToken() },
        body,
      });
      const raw = await res.text();
      const data = JSON.parse(raw);
      if (!res.ok) throw new Error(data?.error ?? res.statusText);
      const n: Note = isForm ? data.note : data;
      setNote(n);
      setMode("note");
      setText("");
      refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setElapsed(0);
    }
  }

  async function toggleRecording() {
    if (recording) {
      recorder.current?.stop();
      setRecording(false);
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const rec = new MediaRecorder(stream, { mimeType: "audio/webm" });
      const chunks: Blob[] = [];
      rec.ondataavailable = (e) => chunks.push(e.data);
      rec.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        const blob = new Blob(chunks, { type: "audio/webm" });
        const form = new FormData();
        form.append("audio", blob, "recap.webm");
        await draft(form, true);
      };
      rec.start();
      recorder.current = rec;
      setRecording(true);
    } catch (e) {
      setError(`microphone unavailable: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  return (
    <main className="mx-auto max-w-6xl px-6 py-10">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="font-[family-name:var(--font-display)] text-[19px] font-semibold tracking-tight lowercase">
            confidant
          </h1>
          <p className="io">
            {demo ? (
              <>
                recorded replay of local runs · {demo.note.model} · read-only, no input accepted
              </>
            ) : (
              <>
                session notes that never leave the room · {health?.runtime.model ?? "local model"} ·{" "}
                {health?.transcriber.available ? `local speech-to-text (${health.transcriber.model})` : "speech-to-text not installed"} · loopback only
              </>
            )}
          </p>
        </div>
        <nav className="flex items-center gap-2">
          {!demo && (
            <button className="quiet" onClick={() => { setMode("new"); setError(null); }}>
              New recap
            </button>
          )}
          {notes.length > 0 && (
            <select
              className="quiet"
              value={note?.id ?? ""}
              onChange={async (e) => {
                if (!e.target.value) return;
                const n = await jsonApi<Note>(`/api/notes/${e.target.value}`);
                setNote(n);
                setMode("note");
              }}
            >
              <option value="">earlier sessions…</option>
              {notes.map((n) => (
                <option key={n.id} value={n.id}>
                  {stamp(n.created_at)} · {n.verified} traced / {n.refused} refused
                </option>
              ))}
            </select>
          )}
        </nav>
      </header>

      <hr className="my-8 border-0 border-t border-[color:var(--color-rule)]" />

      {error && (
        <p className="mb-6 border-l-2 border-[color:var(--color-accent)] pl-3 text-[15px] text-[color:var(--color-accent)]">
          {error}
        </p>
      )}

      {demo ? (
        <DemoView demo={demo} />
      ) : mode === "new" ? (
        <section className="grid gap-10 lg:grid-cols-[1.55fr_1fr]">
          <div>
            <h2 className="label">the twenty seconds after the client leaves</h2>
            <textarea
              className="mt-3 min-h-[13rem] border-b border-[color:var(--color-rule)] text-[19px] leading-relaxed"
              placeholder="Talk or type it the way you would to a colleague: what she brought in, what you noticed, what you agreed to do next. Names and details are fine — nothing here is uploaded."
              value={text}
              onChange={(e) => setText(e.target.value)}
            />
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <button className="primary" disabled={busy || text.trim().length < 20} onClick={() => draft(JSON.stringify({ transcript: text }))}>
                {busy ? `drafting… ${(elapsed / 1000).toFixed(1)}s` : "Draft the note"}
              </button>
              <button className="quiet" disabled={busy || !health?.transcriber.available} onClick={toggleRecording}>
                {recording ? "Stop and draft" : "Record instead"}
              </button>
              <span className="io">
                {text.trim().split(/\s+/).filter(Boolean).length} words · {busy ? "running on this machine" : "nothing sent anywhere"}
              </span>
            </div>
            <p className="io mt-6 max-w-[46ch]">
              The note is drafted here, then every sentence is checked against your own words. Anything the
              model cannot point at in the recap is refused rather than written.
            </p>
          </div>
          <aside className="lg:border-l lg:border-[color:var(--color-rule)] lg:pl-8">
            <h2 className="label">on this machine</h2>
            <ul className="io mt-3 space-y-1">
              <li>model · {health?.runtime.model ?? "unavailable"}</li>
              <li>installed · {(health?.runtime.installed ?? []).join(", ") || "none"}</li>
              <li>outbound network · {health?.runtime.outbound_network === false ? "none" : "unknown"}</li>
              <li>store · a SQLite file in your home directory</li>
              <li>audio · deleted the moment it is transcribed</li>
            </ul>
            <p className="io mt-6">
              Confidant refuses to run against any model host that is not 127.0.0.1. That refusal is a code
              path, not a settings toggle.
            </p>
          </aside>
        </section>
      ) : note ? (
        <NoteView note={note} onSaved={setNote} />
      ) : null}
    </main>
  );
}
