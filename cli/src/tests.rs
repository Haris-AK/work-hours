use super::*;
use tempfile::TempDir;
fn store() -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("db")).unwrap();
    (dir, store)
}
#[test]
fn pause_resume_persistence() {
    let (dir, s) = store();
    s.control("start", 100.0).unwrap();
    s.control("start", 110.0).unwrap();
    let other = Store::open(&dir.path().join("db")).unwrap();
    assert_eq!(other.active().unwrap().unwrap().start, 100.0);
    other.control("pause", 160.0).unwrap();
    s.control("pause", 170.0).unwrap();
    s.control("toggle", 200.0).unwrap();
    s.control("toggle", 230.0).unwrap();
    let total: f64 =
        s.db.query_row("SELECT SUM(end-start) FROM sessions", [], |r| r.get(0))
            .unwrap();
    assert_eq!(total, 90.0);
    assert!(s.active().unwrap().is_none());
}
#[test]
fn corrections_overlap_undo() {
    let (_d, s) = store();
    s.save_session(100.0, 200.0, None, 1000.0).unwrap();
    assert!(s.save_session(150.0, 300.0, None, 1000.0).is_err());
    s.save_session(100.0, 300.0, Some(1), 1000.0).unwrap();
    s.remove(1).unwrap();
    s.restore(1).unwrap();
    s.control("start", 400.0).unwrap();
    assert!(s.save_session(350.0, 500.0, None, 1000.0).is_err());
    assert!(s.save_session(400.0, 500.0, Some(2), 1000.0).is_err());
    assert!(s.save_session(900.0, 1100.0, None, 1000.0).is_err());
    assert!(s.save_session(700.0, 600.0, None, 1000.0).is_err());
    s.remove(1).unwrap();
    s.save_session(100.0, 200.0, None, 1000.0).unwrap();
    assert!(s.restore(1).is_err());
}
#[test]
fn periods() {
    let d = |s: &str| s.parse::<Day>().unwrap();
    assert_eq!(
        period_bounds("week", d("2026-10-14")),
        (d("2026-10-12"), d("2026-10-19"))
    );
    assert_eq!(
        period_bounds("month", d("2024-02-29")),
        (d("2024-02-01"), d("2024-03-01"))
    );
    assert_eq!(
        period_bounds("month", d("2025-12-31")),
        (d("2025-12-01"), d("2026-01-01"))
    );
}
#[test]
fn midnight_running_and_csv() {
    let (dir, s) = store();
    let day: Day = "2026-10-12".parse().unwrap();
    let end = next(next(day));
    let start = midnight(next(day)).unwrap() - 1800.0;
    let finish = start + 3600.0;
    s.control("start", start).unwrap();
    let rows = s.daily(day, end, finish).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.seconds).collect::<Vec<_>>(),
        vec![1800.0, 1800.0]
    );
    s.control("pause", finish).unwrap();
    s.set_note(day, "Fixed \"bug\", reviewed PR\nPlanning")
        .unwrap();
    for kind in ["daily", "sessions"] {
        let path = dir.path().join(format!("{kind}.csv"));
        s.export(&path, day, end, kind, finish).unwrap();
        let mut reader = csv::Reader::from_path(&path).unwrap();
        let headers = reader.headers().unwrap().clone();
        let seconds = headers.iter().position(|h| h == "seconds").unwrap();
        let notes = headers.iter().position(|h| h == "notes").unwrap();
        let rows: Vec<_> = reader.records().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|r| r[seconds].parse::<f64>().unwrap())
                .sum::<f64>(),
            3600.0
        );
        assert_eq!(&rows[0][notes], "Fixed \"bug\", reviewed PR\nPlanning");
        assert!(s.export(&path, day, end, kind, finish).is_err());
    }
}
#[test]
fn notes_append_preserves_previous() {
    let (_d, s) = store();
    let day = today();
    s.append_note(day, "First").unwrap();
    s.append_note(day, "Second").unwrap();
    assert_eq!(s.note(day).unwrap(), "First\nSecond");
}
#[test]
fn original_schema_is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE sessions (id INTEGER PRIMARY KEY,start REAL NOT NULL,end REAL,deleted INTEGER NOT NULL DEFAULT 0,CHECK(end IS NULL OR end >= start)); CREATE UNIQUE INDEX one_running ON sessions((1)) WHERE end IS NULL AND deleted=0; CREATE TABLE notes(day TEXT PRIMARY KEY,text TEXT NOT NULL); INSERT INTO sessions(start) VALUES (123.456); INSERT INTO notes VALUES ('2026-10-12','Keep me');").unwrap();
    drop(db);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.active().unwrap().unwrap().start, 123.456);
    assert_eq!(s.note("2026-10-12".parse().unwrap()).unwrap(), "Keep me");
    s.control("pause", 200.0).unwrap();
}

#[test]
fn idle_pause_is_atomic_idempotent_and_preserves_elapsed_time() {
    let (_dir, s) = store();
    assert!(!s.pause_if_running(100.0).unwrap());
    s.control("start", 100.0).unwrap();
    assert!(s.pause_if_running(1000.0).unwrap());
    assert!(s.active().unwrap().is_none());
    assert!(!s.pause_if_running(2000.0).unwrap());
    let end: f64 =
        s.db.query_row("SELECT end FROM sessions WHERE id=1", [], |r| r.get(0))
            .unwrap();
    assert_eq!(end, 1000.0);
    s.control("start", 2100.0).unwrap();
    assert!(s.pause_if_running(2000.0).unwrap()); // Clock moved backwards.
    let end: f64 =
        s.db.query_row("SELECT end FROM sessions WHERE id=2", [], |r| r.get(0))
            .unwrap();
    assert_eq!(end, 2100.0);
}
