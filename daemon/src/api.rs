//! HTTP surface. Bound to loopback only, and every mutating call needs the run token that
//! the daemon injects into the page it serves — a browser tab from another origin cannot
//! read it, so a random website cannot post a transcript into the local store.

use crate::export;
use crate::note::Note;
use crate::transcribe::Transcriber;
use crate::validator;
use crate::{pipeline::Llm, store, trace};
use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
};
use sentry::protocol::Value;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub llm: Llm,
    pub stt: Transcriber,
    pub db: Mutex<rusqlite::Connection>,
    pub token: String,
    pub ui_dir: std::path::PathBuf,
    /// `Some` when `CONFIDANT_DEMO=1`: the deployment replays recorded runs and takes no input.
    pub demo: Option<Demo>,
}

/// A recorded replay, loaded once at startup: the notes that real local runs produced, plus the
/// newest evaluation table. Nothing here is generated on the public deployment.
#[derive(Clone)]
pub struct Demo {
    pub notes: Vec<Note>,
    pub eval: Option<serde_json::Value>,
    pub eval_file: Option<String>,
}

impl Demo {
    /// Load `demo/note.json`, `demo/note-b.json` (if present) and the newest `eval/results/*.json`
    /// under `root`. A missing artifact is logged and left out; it is never invented.
    pub fn load(root: &std::path::Path) -> Demo {
        let dir = std::env::var("CONFIDANT_DEMO_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| root.join("demo"));
        let mut notes = Vec::new();
        for name in ["note.json", "note-b.json"] {
            let path = dir.join(name);
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<Note>(&s).map_err(|e| e.to_string()))
            {
                Ok(n) => notes.push(n),
                Err(e) => tracing::warn!("demo: {} not loaded: {e}", path.display()),
            }
        }
        let (eval, eval_file) = newest_eval(root);
        tracing::info!(
            notes = notes.len(),
            eval = eval_file.as_deref().unwrap_or("none"),
            "demo mode: replaying recorded runs; every mutating route answers 403"
        );
        Demo { notes, eval, eval_file }
    }

    /// The public replay payload. `read_only` is not a promise in prose — the middleware in
    /// `demo_guard` is what makes it true.
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({
            "note": self.notes.first(),
            "note_b": self.notes.get(1),
            "eval": self.eval,
            "eval_file": self.eval_file,
            "mode": "demo",
            "read_only": true,
            "provenance": "recorded on the author's laptop by real runs of this daemon against a local model; this deployment replays them and accepts no input",
        })
    }

    pub fn models(&self) -> Vec<String> {
        let mut out: Vec<String> = self.notes.iter().map(|n| n.model.clone()).collect();
        out.sort();
        out.dedup();
        out
    }
}

/// The newest `eval/results/*.json`, by mtime.
///
/// A fresh git checkout gives every file the same timestamp, and readdir order is not stable
/// across container starts — so ties are broken by path. Without that, the same image can serve
/// a different evaluation table after a restart, which is exactly the kind of drift a demo that
/// claims to replay a recording cannot have.
fn newest_eval(root: &std::path::Path) -> (Option<serde_json::Value>, Option<String>) {
    let dir = root.join("eval/results");
    let mut candidates: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(meta) = entry.metadata() {
                    candidates.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), path));
                }
            }
        }
    }
    match pick_newest(candidates) {
        Some(path) => {
            let name = path.file_name().map(|f| f.to_string_lossy().to_string());
            match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()) {
                Some(v) => (Some(v), name),
                None => (None, None),
            }
        }
        None => (None, None),
    }
}

/// Pure so the tie-break can be tested without touching the filesystem.
fn pick_newest(mut candidates: Vec<(std::time::SystemTime, std::path::PathBuf)>) -> Option<std::path::PathBuf> {
    candidates.sort();
    candidates.pop().map(|(_, path)| path)
}

fn authorised(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get("x-confidant-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == state.token)
        .unwrap_or(false)
}

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (code, Json(serde_json::json!({ "error": msg.into() })))
}

/// Serve the app shell with this run's token injected.
pub async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let path = state.ui_dir.join("index.html");
    match std::fs::read_to_string(&path) {
        Ok(html) => Html(html.replace("__CONFIDANT_TOKEN__", &state.token)),
        Err(e) => Html(format!(
            "<!doctype html><meta charset=utf-8><pre style=\"font:14px ui-monospace;padding:2rem\">Confidant daemon is up, but the UI build is missing at {}.\nRun: cd ui && npm run build\n\n({e})</pre>",
            path.display()
        )),
    }
}

pub async fn health(State(state): State<Arc<AppState>>) -> axum::response::Response {
    if let Some(d) = &state.demo {
        return Json(serde_json::json!({
            "ok": true,
            "mode": "demo",
            "leaves_machine": false,
            "local_only": true,
            "read_only": true,
            "runtime": { "mode": "demo", "model": d.models().join(", "), "note": "recorded runs; no model is served on this deployment" },
            "transcriber": { "available": false, "model": "not deployed" },
        }))
        .into_response();
    }
    let runtime = state.llm.health().await.unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }));
    let local = runtime["mode"] == "local";
    Json(serde_json::json!({
        "ok": runtime.get("error").is_none(),
        "mode": runtime["mode"],
        "leaves_machine": runtime["leaves_machine"],
        "local_only": local,
        "runtime": runtime,
        "transcriber": { "available": state.stt.available(), "model": state.stt.model },
    }))
    .into_response()
}

/// The whole public surface of a demo deployment: the recorded runs and the eval table.
pub async fn demo(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match &state.demo {
        Some(d) => (StatusCode::OK, Json(d.body())).into_response(),
        None => err(StatusCode::NOT_FOUND, "not a demo deployment").into_response(),
    }
}

/// Demo deployments replay; they never take input. This runs in front of every handler, so a
/// mutating request is refused before it can reach a route — the read-only claim is code, not copy.
pub async fn demo_guard(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::Method;
    if state.demo.is_some() && !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return err(
            StatusCode::FORBIDDEN,
            "demo mode: this deployment replays recorded runs and accepts no input (nothing here is a live service)",
        )
        .into_response();
    }
    next.run(req).await
}

pub async fn list_notes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match store::list(&db, 50) {
        Ok(notes) => {
            let brief: Vec<_> = notes
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "id": n.id, "created_at": n.created_at, "model": n.model,
                        "verified": n.stats.claims_verified, "refused": n.stats.claims_unverified,
                        "first_line": n.draft.subjective.first().or(n.draft.objective.first())
                            .map(|c| c.text.clone()).unwrap_or_default()
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "notes": brief }))).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn get_note(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match store::get(&db, &id) {
        Ok(Some(n)) => (StatusCode::OK, Json(n)).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such note").into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TextRecap {
    pub transcript: String,
    /// Optional: which installed model to draft with. Defaults to the daemon's model.
    #[serde(default)]
    pub model: Option<String>,
}

pub async fn models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(d) = &state.demo {
        let list: Vec<serde_json::Value> = d.models().iter().map(|m| serde_json::json!({ "name": m })).collect();
        return (StatusCode::OK, Json(serde_json::json!({ "mode": "demo", "models": list, "selected": null }))).into_response();
    }
    match state.llm.models().await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

pub async fn recap_text(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(body): Json<TextRecap>) -> impl IntoResponse {
    if !authorised(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing run token").into_response();
    }
    match run_pipeline(&state, body.transcript, 0, body.model).await {
        Ok(note) => (StatusCode::OK, Json(note)).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

pub async fn recap_audio(State(state): State<Arc<AppState>>, headers: HeaderMap, mut multipart: Multipart) -> impl IntoResponse {
    if !authorised(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing run token").into_response();
    }
    let mut audio: Option<std::path::PathBuf> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("audio") {
            let bytes = match field.bytes().await {
                Ok(b) => b,
                Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
            };
            let dir = std::env::temp_dir().join("confidant-audio");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
            let path = dir.join(format!("{}.wav", uuid::Uuid::new_v4()));
            if let Err(e) = std::fs::write(&path, &bytes) {
                return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
            audio = Some(path);
        }
    }
    let Some(path) = audio else {
        return err(StatusCode::BAD_REQUEST, "expected a multipart field named 'audio'").into_response();
    };
    let tx = trace::transaction("note.from_audio");
    let stt_span = tx.as_ref().map(|t| t.start_child("stt.transcribe", "stt"));
    let stt = crate::trace::Stage::start("stt.transcribe");
    let result = state.stt.run(&path);
    let stt_ms = stt.finish();
    if let Ok(t) = &result {
        tracing::info!(stt_ms, sidecar_ms = t.ms, segments = t.segments, chars = t.text.len(), model = %t.model, "transcribed locally, audio deleted");
    }
    if let Some(s) = stt_span {
        s.set_data("duration_ms", Value::from(stt_ms));
        s.finish();
    }
    let outcome = match result {
        Ok(t) => {
            let stt_model = t.model.clone();
            match run_pipeline(&state, t.text, stt_ms, None).await {
                Ok(note) => Ok((note, stt_model)),
                Err(e) => Err(err(StatusCode::BAD_GATEWAY, e.to_string())),
            }
        }
        Err(e) => Err(err(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())),
    };
    if let Some(t) = tx {
        if let Ok((_, m)) = &outcome {
            t.set_tag("stt.model", m.clone());
        }
        t.set_data("stt_ms", Value::from(stt_ms));
        t.finish();
    }
    match outcome {
        Ok((note, _)) => (StatusCode::OK, Json(serde_json::json!({ "note": note, "stt_ms": stt_ms }))).into_response(),
        Err(r) => r.into_response(),
    }
}

#[derive(Deserialize)]
pub struct PatchNote {
    pub section: String,
    pub index: usize,
    pub text: String,
}

/// Editing is the clinician's: a human edit is trusted, so it clears the claim's span and is
/// marked as clinician-written by keeping the original wording in `quote` untouched.
pub async fn patch_note(State(state): State<Arc<AppState>>, headers: HeaderMap, AxumPath(id): AxumPath<String>, Json(p): Json<PatchNote>) -> impl IntoResponse {
    if !authorised(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing run token").into_response();
    }
    let db = state.db.lock().unwrap();
    let mut note = match store::get(&db, &id) {
        Ok(Some(n)) => n,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such note").into_response(),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let section = match p.section.as_str() {
        "subjective" => &mut note.draft.subjective,
        "objective" => &mut note.draft.objective,
        "assessment" => &mut note.draft.assessment,
        "plan" => &mut note.draft.plan,
        "next_focus" => &mut note.draft.next_focus,
        _ => return err(StatusCode::BAD_REQUEST, "unknown section").into_response(),
    };
    match section.get_mut(p.index) {
        Some(c) => {
            c.text = p.text;
            c.quote = format!("clinician edit: {}", c.quote);
            c.span = None;
        }
        None => return err(StatusCode::BAD_REQUEST, "index out of range").into_response(),
    }
    if let Err(e) = store::update(&db, &note) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    (StatusCode::OK, Json(note)).into_response()
}

pub async fn delete_note(State(state): State<Arc<AppState>>, headers: HeaderMap, AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    if !authorised(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing run token").into_response();
    }
    let db = state.db.lock().unwrap();
    match db.execute("DELETE FROM notes WHERE id = ?1", rusqlite::params![id]) {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
}

pub async fn export_note(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>, Query(q): Query<ExportQuery>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match store::get(&db, &id) {
        Ok(Some(n)) => match q.format.as_deref().unwrap_or("md") {
            "html" => (
                StatusCode::OK,
                [("content-type", "text/html; charset=utf-8")],
                export::to_html(&n),
            )
                .into_response(),
            _ => (
                StatusCode::OK,
                [("content-type", "text/markdown; charset=utf-8")],
                export::to_markdown(&n),
            )
                .into_response(),
        },
        Ok(None) => err(StatusCode::NOT_FOUND, "no such note").into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// The pipeline: draft (local model) → verify (deterministic) → store.
pub async fn run_pipeline(
    state: &AppState,
    transcript: String,
    stt_ms: u64,
    model_override: Option<String>,
) -> anyhow::Result<Note> {
    let tx = trace::transaction("note.draft");
    state.llm.assert_policy()?;
    let llm = match model_override {
        Some(m) if !m.trim().is_empty() => state.llm.with_model(m),
        _ => state.llm.clone(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let transcript = transcript.trim().to_string();

    let gen = trace::Stage::start("llm.generate");
    let gen_span = tx.as_ref().map(|t| t.start_child("llm.generate", "llm"));
    let (draft, mut stats) = llm.draft_note(&transcript).await?;
    let gen_ms = gen.finish();
    if let Some(s) = gen_span {
        s.set_data("output_tokens", Value::from(stats.output_tokens));
        s.set_data("duration_ms", Value::from(gen_ms));
        s.set_data("cost_usd", Value::from(0.0f64));
        s.finish();
    }

    let verify = trace::Stage::start("guardrail.verify");
    let verified = validator::verify(&transcript, draft);
    let verify_ms = verify.finish();

    stats.claims_total = verified.stats_partial.claims_total;
    stats.claims_verified = verified.stats_partial.claims_verified;
    stats.claims_unverified = verified.stats_partial.claims_unverified;
    stats.spans = verified.stats_partial.spans;
    stats.gen_ms = gen_ms;
    stats.verify_ms = verify_ms;
    stats.stt_ms = stt_ms;

    let note = Note {
        id: id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        model: llm.model.clone(),
        transcript,
        draft: verified.draft,
        risk_flags: verified.risk_flags,
        unverified: verified.unverified,
        stats,
    };

    if let Some(s) = tx.as_ref().map(|t| t.start_child("store.write", "db")) {
        s.set_data("transcript_chars", Value::from(note.transcript.len() as i64));
        s.finish();
    }
    {
        let db = state.db.lock().unwrap();
        store::insert(&db, &note)?;
    }
    tracing::info!(
        note_id = %id,
        gen_ms,
        verify_ms,
        verified = note.stats.claims_verified,
        refused = note.stats.claims_unverified,
        "note ready: text never left the machine"
    );
    if let Some(t) = tx {
        t.set_tag("model", note.model.clone());
        t.finish();
    }
    Ok(note)
}

/// Kept next to the handlers so the routes and the test suite cannot drift apart.
pub fn routes() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/api/health", get(health))
        .route("/api/models", get(models))
        .route("/api/demo", get(demo))
        .route("/api/notes", get(list_notes))
        .route("/api/notes/{id}", get(get_note).patch(patch_note).delete(delete_note))
        .route("/api/notes/{id}/export", get(export_note))
        .route("/api/recap/text", post(recap_text))
        .route("/api/recap/audio", post(recap_audio))
        .route("/", get(index))
        .route("/app", get(index))
}

/// The demo mode is a promise to a stranger on the internet ("this page takes no input"), so it
/// is tested through the real router and the real middleware, not by calling handlers directly.
#[cfg(test)]
mod demo_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    fn demo_state() -> Arc<AppState> {
        let dir = std::env::temp_dir().join(format!("confidant-demo-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = store::open(&dir.join("t.db")).unwrap();
        let note = Note {
            id: "demo-1".into(),
            created_at: "2026-10-13T12:00:00Z".into(),
            model: "gemma3:1b-it-qat".into(),
            transcript: "we agreed she would write the message".into(),
            draft: crate::note::DraftNote {
                subjective: vec![crate::note::Claim {
                    text: "Slept six hours most nights.".into(),
                    quote: "she slept maybe six hours most nights".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        Arc::new(AppState {
            llm: Llm::from_env(),
            stt: Transcriber::from_env(),
            db: Mutex::new(db),
            token: "demo".into(),
            ui_dir: dir,
            demo: Some(Demo {
                notes: vec![note],
                eval: Some(serde_json::json!({ "summary": { "arm": "rules-only", "recaps": 8 } })),
                eval_file: Some("eval-rules-only.json".into()),
            }),
        })
    }

    async fn call(state: Arc<AppState>, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let app = routes()
            .layer(axum::middleware::from_fn_with_state(state.clone(), demo_guard))
            .with_state(state);
        let res = app.oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::json!({})))
    }

    #[tokio::test]
    async fn mutating_routes_are_refused_in_demo_mode() {
        for (method, path) in [
            (Method::POST, "/api/recap/text"),
            (Method::POST, "/api/recap/audio"),
            (Method::PATCH, "/api/notes/demo-1"),
            (Method::DELETE, "/api/notes/demo-1"),
        ] {
            let req = Request::builder()
                .method(method.clone())
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from("{\"transcript\":\"a client's words\"}"))
                .unwrap();
            let (status, body) = call(demo_state(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} must be refused");
            assert!(
                body["error"].as_str().unwrap_or("").contains("accepts no input"),
                "{method} {path} must say why: {body}"
            );
        }
    }

    #[tokio::test]
    async fn demo_endpoint_is_read_only_and_serves_the_recording() {
        let req = Request::builder().uri("/api/demo").body(Body::empty()).unwrap();
        let (status, body) = call(demo_state(), req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["read_only"], true);
        assert_eq!(body["mode"], "demo");
        assert_eq!(body["note"]["id"], "demo-1");
        assert_eq!(body["eval"]["summary"]["recaps"], 8);
    }

    #[tokio::test]
    async fn reads_still_work_in_demo_mode() {
        let req = Request::builder().uri("/api/health").body(Body::empty()).unwrap();
        let (status, body) = call(demo_state(), req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["mode"], "demo");
        assert_eq!(body["read_only"], true);
    }

    #[test]
    fn equal_timestamps_pick_the_same_file_whatever_the_order() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
        let a = (t, std::path::PathBuf::from("eval-a.json"));
        let b = (t, std::path::PathBuf::from("eval-b.json"));
        let forward = pick_newest(vec![a.clone(), b.clone()]).unwrap();
        let backward = pick_newest(vec![b, a]).unwrap();
        assert_eq!(forward, backward, "a git checkout ties every mtime; the choice must not depend on readdir order");
        assert_eq!(forward, std::path::PathBuf::from("eval-b.json"));
    }

    #[test]
    fn a_newer_file_beats_a_name_that_sorts_later() {
        let t0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
        let t1 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(2);
        let picked = pick_newest(vec![
            (t1, std::path::PathBuf::from("eval-a.json")),
            (t0, std::path::PathBuf::from("eval-z.json")),
        ])
        .unwrap();
        assert_eq!(picked, std::path::PathBuf::from("eval-a.json"));
    }
}
