//! Local store. One SQLite file on the clinician's disk. No migrations server, no sync.

use crate::note::Note;
use anyhow::Result;
use rusqlite::{params, Connection};

pub fn open(path: &std::path::Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS notes (
            id         TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            model      TEXT NOT NULL,
            transcript TEXT NOT NULL,
            note_json  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS notes_created ON notes(created_at DESC);
        "#,
    )?;
    Ok(conn)
}

pub fn insert(conn: &Connection, note: &Note) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO notes (id, created_at, model, transcript, note_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![note.id, note.created_at, note.model, note.transcript, serde_json::to_string(note)?],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Note>> {
    let mut st = conn.prepare("SELECT note_json FROM notes WHERE id = ?1")?;
    let mut rows = st.query(params![id])?;
    if let Some(row) = rows.next()? {
        let s: String = row.get(0)?;
        return Ok(Some(serde_json::from_str(&s)?));
    }
    Ok(None)
}

pub fn list(conn: &Connection, limit: i64) -> Result<Vec<Note>> {
    let mut st = conn.prepare("SELECT note_json FROM notes ORDER BY created_at DESC LIMIT ?1")?;
    let rows = st.query_map(params![limit], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        let s = r?;
        out.push(serde_json::from_str(&s)?);
    }
    Ok(out)
}

pub fn update(conn: &Connection, note: &Note) -> Result<()> {
    conn.execute("UPDATE notes SET note_json = ?2 WHERE id = ?1", params![note.id, serde_json::to_string(note)?])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Note;

    #[test]
    fn round_trips_a_note() {
        let dir = std::env::temp_dir().join(format!("confidant-test-{}", uuid::Uuid::new_v4()));
        let conn = open(&dir.join("t.db")).unwrap();
        let n = Note { id: "n1".into(), created_at: "2026-10-13T12:00:00Z".into(), model: "m".into(), transcript: "t".into(), ..Default::default() };
        insert(&conn, &n).unwrap();
        assert_eq!(get(&conn, "n1").unwrap().unwrap().id, "n1");
        assert_eq!(list(&conn, 10).unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
