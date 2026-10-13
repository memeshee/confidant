//! Confidant daemon — local-first session-note drafter.
//!
//! Binds 127.0.0.1 only. The model runs on this machine; the transcript never travels.
//! `--selftest` runs the pipeline against a fixture transcript and prints a JSON summary,
//! which is what the eval harness and the judge-facing demo both call.

mod api;
mod export;
mod note;
mod pipeline;
mod store;
mod trace;
mod transcribe;
mod validator;

use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,confidant_daemon=info".into()))
        .with_target(false)
        .init();
    let _sentry = trace::init();

    let root = std::env::var("CONFIDANT_ROOT").unwrap_or_else(|_| ".".into());
    // `CONFIDANT_DEMO=1`: the public, read-only deployment. It replays recorded runs and refuses
    // every mutating request; see `api::demo_guard`.
    let demo_mode = std::env::var("CONFIDANT_DEMO").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let db_path = std::env::var("CONFIDANT_DB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            if demo_mode {
                std::env::temp_dir().join("confidant-demo.db")
            } else {
                dirs_data().join("confidant/notes.db")
            }
        });
    let ui_dir = std::path::PathBuf::from(std::env::var("CONFIDANT_UI").unwrap_or_else(|_| format!("{root}/ui/out")));
    // Render hands the port in $PORT; CONFIDANT_PORT still wins for local runs.
    let port: u16 = std::env::var("CONFIDANT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8787);
    // Loopback by default: normally this daemon is a private tool. A demo deployment holds only
    // recorded, read-only material and must answer the platform's router, so it binds all interfaces.
    let bind = std::env::var("CONFIDANT_BIND").unwrap_or_else(|_| if demo_mode { "0.0.0.0".into() } else { "127.0.0.1".into() });

    let llm = pipeline::Llm::from_env();
    llm.assert_policy()?;
    if !llm.is_local() {
        tracing::warn!(
            "REMOTE MODE: drafts are sent to {}. The guardrail still runs locally, but the transcript leaves this machine.",
            llm.label()
        );
    }

    let demo = if demo_mode {
        let d = api::Demo::load(std::path::Path::new(&root));
        tracing::info!(
            "DEMO MODE: {} recorded run(s) and the eval table will be served read-only; every POST/PATCH/DELETE answers 403",
            d.notes.len()
        );
        Some(d)
    } else {
        None
    };

    // `--selftest <transcript-file>`: one real pass through the pipeline, JSON on stdout.
    if let Some(idx) = std::env::args().position(|a| a == "--selftest") {
        let file = std::env::args().nth(idx + 1).ok_or_else(|| anyhow::anyhow!("usage: confidant-daemon --selftest <transcript.txt>"))?;
        let transcript = std::fs::read_to_string(&file)?;
        let conn = store::open(&db_path)?;
        let state = Arc::new(api::AppState { llm, stt: transcribe::Transcriber::from_env(), db: Mutex::new(conn), token: "selftest".into(), ui_dir, demo });
        let t0 = std::time::Instant::now();
        let note = api::run_pipeline(&state, transcript, 0, std::env::var("CONFIDANT_MODEL").ok()).await?;
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "note_id": note.id,
            "model": note.model,
            "wall_ms": t0.elapsed().as_millis() as u64,
            "stats": note.stats,
            "risk_flags": note.risk_flags,
            "sections": {
                "subjective": note.draft.subjective.len(),
                "objective": note.draft.objective.len(),
                "assessment": note.draft.assessment.len(),
                "plan": note.draft.plan.len(),
                "next_focus": note.draft.next_focus.len(),
            },
            "refused": note.unverified.iter().map(|c| c.text.clone()).collect::<Vec<_>>(),
        }))?);
        return Ok(());
    }

    let token = match std::env::var("CONFIDANT_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            let t: String = (0..32).map(|_| {
                let c = rand::random::<u8>() % 62;
                (b'0' + if c < 10 { c } else if c < 36 { c + 39 } else { c + 61 }) as char
            }).collect();
            t
        }
    };
    let conn = store::open(&db_path)?;
    // In demo mode the recorded notes go into the (throwaway) store so the ordinary read paths —
    // list, get, markdown/HTML export — work exactly as they do on a clinician's machine.
    if let Some(d) = &demo {
        for note in &d.notes {
            store::insert(&conn, note)?;
        }
    }
    let state = Arc::new(api::AppState {
        llm,
        stt: transcribe::Transcriber::from_env(),
        db: Mutex::new(conn),
        token,
        ui_dir: ui_dir.clone(),
        demo,
    });

    let app = api::routes()
        .layer(axum::middleware::from_fn_with_state(state.clone(), api::demo_guard))
        .with_state(state.clone())
        .fallback_service(tower_http::services::ServeDir::new(&ui_dir));

    let addr = std::net::SocketAddr::from((bind.parse::<std::net::IpAddr>()?, port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Confidant on http://{addr} · {} · store {}", state.llm.label(), db_path.display());
    if !ui_dir.join("index.html").exists() {
        tracing::warn!("UI not built yet ({} missing) — run: cd ui && npm run build", ui_dir.join("index.html").display());
    }
    axum::serve(listener, app).await?;
    Ok(())
}

fn dirs_data() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local/share")
}
