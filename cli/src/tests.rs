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
    assert!(!s.pause_if_running(100.0, 0.0).unwrap());
    s.control("start", 100.0).unwrap();
    assert!(s.pause_if_running(1000.0, 0.0).unwrap());
    assert!(s.active().unwrap().is_none());
    assert!(!s.pause_if_running(2000.0, 0.0).unwrap());
    let end: f64 =
        s.db.query_row("SELECT end FROM sessions WHERE id=1", [], |r| r.get(0))
            .unwrap();
    assert_eq!(end, 1000.0);
    s.control("start", 2100.0).unwrap();
    assert!(s.pause_if_running(2000.0, 0.0).unwrap()); // Clock moved backwards.
    let end: f64 =
        s.db.query_row("SELECT end FROM sessions WHERE id=2", [], |r| r.get(0))
            .unwrap();
    assert_eq!(end, 2100.0);
}

#[test]
fn idle_pause_excludes_the_idle_period_but_not_time_before_the_start() {
    let (_dir, s) = store();
    s.control("start", 100.0).unwrap();
    assert!(s.pause_if_running(2000.0, 900.0).unwrap());
    s.control("start", 3000.0).unwrap();
    assert!(s.pause_if_running(3500.0, 900.0).unwrap());
    let ends: Vec<f64> =
        s.db.prepare("SELECT end FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
    assert_eq!(ends, vec![1100.0, 3000.0]);
}

fn here(boot: &str, slept: f64, login: Option<&str>) -> Presence {
    Presence {
        boot: boot.into(),
        slept,
        login: login.map(Into::into),
    }
}
fn started(s: &Store, at: f64, presence: &Presence) {
    s.db.execute("INSERT INTO sessions(start) VALUES (?)", [at])
        .unwrap();
    s.db.execute("DELETE FROM presence", []).unwrap();
    assert_eq!(s.catch_up(at, presence).unwrap(), None);
}
fn end_of(s: &Store) -> Option<f64> {
    s.db.query_row(
        "SELECT end FROM sessions ORDER BY id DESC LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn sleep_is_removed_and_the_timer_stays_paused_on_wake() {
    let (_dir, s) = store();
    started(&s, 1000.0, &here("a", 5.0, None));
    assert_eq!(s.catch_up(1600.0, &here("a", 5.0, None)).unwrap(), None);
    // Asleep for an hour, then woke 2 seconds before this poll.
    let caught = s
        .catch_up(1600.0 + 3602.0, &here("a", 3605.0, None))
        .unwrap();
    assert_eq!(caught, Some((Away::Asleep, 1602.0)));
    assert!(s.active().unwrap().is_none());
    assert_eq!(s.catch_up(9000.0, &here("a", 9000.0, None)).unwrap(), None);
}

#[test]
fn short_suspends_and_a_stopped_bar_do_not_cut_sessions() {
    let (_dir, s) = store();
    started(&s, 1000.0, &here("a", 0.0, None));
    assert_eq!(s.catch_up(1100.0, &here("a", 30.0, None)).unwrap(), None);
    // No observations for hours (bar closed), but the computer stayed awake and logged in.
    assert_eq!(s.catch_up(20000.0, &here("a", 30.0, None)).unwrap(), None);
    assert!(s.active().unwrap().is_some());
}

#[test]
fn restart_ends_at_the_last_observation() {
    let (_dir, s) = store();
    started(&s, 1000.0, &here("a", 0.0, None));
    s.catch_up(1500.0, &here("a", 0.0, None)).unwrap();
    s.catch_up(1505.0, &here("a", 0.0, None)).unwrap(); // Within the heartbeat interval.
    let caught = s.catch_up(50000.0, &here("b", 0.0, None)).unwrap();
    assert_eq!(caught, Some((Away::Restarted, 1500.0)));
    assert_eq!(end_of(&s), Some(1500.0));
}

#[test]
fn logout_is_detected_only_by_the_graphical_session() {
    let (_dir, s) = store();
    started(&s, 1000.0, &here("a", 0.0, None));
    s.catch_up(1001.0, &here("a", 0.0, Some("3"))).unwrap();
    // A terminal or SSH command reports no login and must not end the session.
    assert_eq!(s.catch_up(1100.0, &here("a", 0.0, None)).unwrap(), None);
    let caught = s.catch_up(5000.0, &here("a", 0.0, Some("7"))).unwrap();
    assert_eq!(caught, Some((Away::LoggedOut, 1100.0)));
}

#[test]
fn sessions_from_before_the_upgrade_are_adopted_not_cut() {
    let (_dir, s) = store();
    s.db.execute("INSERT INTO sessions(start) VALUES (100.0)", [])
        .unwrap();
    assert_eq!(
        s.catch_up(90000.0, &here("z", 99999.0, None)).unwrap(),
        None
    );
    assert!(s.active().unwrap().is_some());
}
