//! Observability. Metadata only: stage names, durations, token counts and a random note id.
//! Clinical text never enters a span, an event, or a breadcrumb — this module has no API
//! that accepts text, which is the only version of that claim that survives a code review.

use std::time::Instant;

pub fn init() -> Option<sentry::ClientInitGuard> {
    match std::env::var("SENTRY_DSN") {
        Ok(dsn) if !dsn.trim().is_empty() => {
            let guard = sentry::init((
                dsn,
                sentry::ClientOptions {
                    release: sentry::release_name!(),
                    traces_sample_rate: 1.0,
                    send_default_pii: false,
                    ..Default::default()
                },
            ));
            tracing::info!("sentry tracing on: metadata only, no clinical text");
            Some(guard)
        }
        _ => {
            tracing::info!("SENTRY_DSN unset: sentry disabled (local stage timings still recorded)");
            None
        }
    }
}

pub fn transaction(name: &'static str) -> Option<sentry::Transaction> {
    if sentry::Hub::current().client().is_some() {
        Some(sentry::start_transaction(sentry::TransactionContext::new(name, "pipeline")))
    } else {
        None
    }
}

/// A local stage timer. Always cheap; used for the in-app trace panel and the log line.
/// Sentry child spans are opened inline where the transaction is in scope (see `api.rs`),
/// so nothing here needs to hold a borrowed span.
pub struct Stage {
    pub name: &'static str,
    started: Instant,
}

impl Stage {
    pub fn start(name: &'static str) -> Self {
        Self { name, started: Instant::now() }
    }

    pub fn ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn finish(self) -> u64 {
        let ms = self.ms();
        tracing::info!(stage = self.name, ms, "stage complete");
        ms
    }
}
