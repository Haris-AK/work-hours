//! End-to-end tests of the actual CLI, with isolated databases and child environments.
use chrono::DateTime;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

struct Tracker {
    directory: TempDir,
    db: PathBuf,
}
impl Tracker {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("data.sqlite3");
        let tracker = Self { directory, db };
        tracker.run(&["status"]);
        tracker
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_work-hours"));
        command
            .args(args)
            .arg("--db")
            .arg(&self.db)
            .env("TZ", "Europe/London");
        command
    }
    fn checked(output: Output) -> String {
        assert!(
            output.status.success(),
            "CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn run(&self, args: &[&str]) -> String {
        Self::checked(self.command(args).output().unwrap())
    }
    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.run(args)).unwrap()
    }
    fn connection(&self) -> Connection {
        Connection::open(&self.db).unwrap()
    }
    fn add(&self, start: &str, end: &str) {
        let stamp = |value: &str| DateTime::parse_from_rfc3339(value).unwrap().timestamp() as f64;
        self.connection()
            .execute(
                "INSERT INTO sessions(start,end) VALUES (?,?)",
                params![stamp(start), stamp(end)],
            )
            .unwrap();
    }
    fn concurrent(&self, args: Vec<Vec<String>>) {
        let children: Vec<_> = args
            .iter()
            .map(|args| {
                self.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in children {
            Self::checked(child.wait_with_output().unwrap());
        }
    }
}

#[test]
fn controls_and_bar_show_state_and_seconds() {
    let tracker = Tracker::new();
    assert_eq!(tracker.run(&["start"]).trim(), "Working");
    assert_eq!(tracker.run(&["start"]).trim(), "Working");
    let bar = tracker.json(&["bar"]);
    assert_eq!(bar["class"], "running");
    let text = bar["text"]
        .as_str()
        .unwrap()
        .strip_prefix("▶ Working · ")
        .unwrap();
    let fields: Vec<_> = text.split_whitespace().collect();
    assert_eq!(fields.len(), 3);
    for (field, suffix) in fields.iter().zip(['h', 'm', 's']) {
        assert!(field.strip_suffix(suffix).unwrap().parse::<u64>().is_ok());
    }
    assert_eq!(fields[1].len(), 3);
    assert_eq!(fields[2].len(), 3);
    assert_eq!(tracker.run(&["toggle"]).trim(), "Paused");
    assert_eq!(tracker.run(&["pause"]).trim(), "Paused");
    let bar = tracker.json(&["bar"]);
    assert_eq!(bar["class"], "paused");
    assert!(bar["text"].as_str().unwrap().starts_with("Ⅱ Paused · "));
    let count: i64 = tracker
        .connection()
        .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn idle_pause_notifies_only_when_a_running_timer_is_paused() {
    let tracker = Tracker::new();
    let notifier = tracker.directory.path().join("notify-send");
    let log = tracker.directory.path().join("notifications");
    fs::write(
        &notifier,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$WORK_HOURS_NOTIFICATION_LOG\"\n",
    )
    .unwrap();
    fs::set_permissions(&notifier, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(tracker.directory.path().to_path_buf()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap();
    let idle = || {
        Tracker::checked(
            tracker
                .command(&["idle-pause"])
                .env("PATH", &path)
                .env("WORK_HOURS_NOTIFICATION_LOG", &log)
                .output()
                .unwrap(),
        );
    };
    idle();
    assert!(!log.exists());
    tracker.run(&["start"]);
    idle();
    assert_eq!(tracker.json(&["bar"])["class"], "paused");
    let notification = fs::read_to_string(&log).unwrap();
    assert!(notification.contains("Work Hours paused"));
    assert!(notification.contains("No activity for 15 minutes."));
    idle();
    assert_eq!(fs::read_to_string(&log).unwrap(), notification);
}

#[test]
fn dst_summaries_use_actual_elapsed_seconds() {
    let tracker = Tracker::new();
    for (start, end, hours) in [
        ("2026-03-29T00:00:00+00:00", "2026-03-30T00:00:00+01:00", 23),
        ("2025-10-26T00:00:00+01:00", "2025-10-27T00:00:00+00:00", 25),
    ] {
        tracker.add(start, end);
        let output = tracker.run(&["summary", "--period", "day", "--date", &start[..10]]);
        assert!(
            output.contains(&format!("Total: {hours}h 00m 00s")),
            "{output}"
        );
    }
}

#[test]
fn csv_preserves_notes_splits_midnight_and_refuses_overwrite() {
    let tracker = Tracker::new();
    tracker.add("2026-10-12T23:30:00+01:00", "2026-10-13T00:30:00+01:00");
    let note = "Fixed \"bug\", reviewed PR\nPlanning";
    tracker.run(&["note", "--date", "2026-10-12", "--text", note]);
    tracker.run(&["note", "--date", "2026-10-12", "--text", "More work"]);
    for kind in ["daily", "sessions"] {
        let path = tracker.directory.path().join(format!("{kind}.csv"));
        let args = [
            "export",
            "--from",
            "2026-10-12",
            "--to",
            "2026-10-13",
            "--kind",
            kind,
            "--output",
            path.to_str().unwrap(),
        ];
        tracker.run(&args);
        let contents = fs::read(&path).unwrap();
        let mut reader = csv::Reader::from_reader(contents.as_slice());
        let headers = reader.headers().unwrap().clone();
        let seconds = headers.iter().position(|s| s == "seconds").unwrap();
        let notes = headers.iter().position(|s| s == "notes").unwrap();
        let rows: Vec<_> = reader.records().map(Result::unwrap).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| row[seconds].parse::<f64>().unwrap())
                .sum::<f64>(),
            3600.0
        );
        assert_eq!(&rows[0][notes], format!("{note}\nMore work"));
        assert!(!tracker.command(&args).output().unwrap().status.success());
        assert_eq!(fs::read(&path).unwrap(), contents);
    }
}

#[test]
fn concurrent_start_and_note_append_do_not_duplicate_or_lose_data() {
    let tracker = Tracker::new();
    tracker.concurrent(vec![vec!["start".to_owned()]; 8]);
    let count: i64 = tracker
        .connection()
        .query_row(
            "SELECT count(*) FROM sessions WHERE end IS NULL AND deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    tracker.concurrent(
        (0..8)
            .map(|i| {
                vec![
                    "note".into(),
                    "--date".into(),
                    "2026-10-12".into(),
                    "--text".into(),
                    format!("Line {i}"),
                ]
            })
            .collect(),
    );
    let text = tracker.run(&["note", "--date", "2026-10-12"]);
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 8);
    assert_eq!(
        lines
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        (0..8).map(|i| format!("Line {i}")).collect()
    );
}

#[test]
fn historical_sessions_can_be_corrected_without_changing_their_id() {
    let tracker = Tracker::new();
    tracker.run(&[
        "session-save",
        "--start",
        "2026-10-12T09:00:00+01:00",
        "--end",
        "2026-10-12T10:00:00+01:00",
    ]);
    let snapshot = tracker.json(&["snapshot", "--period", "day", "--date", "2026-10-12"]);
    let id = snapshot["sessions"][0]["id"].as_i64().unwrap().to_string();
    tracker.run(&[
        "session-save",
        "--id",
        &id,
        "--start",
        "2026-10-12T08:30:00+01:00",
        "--end",
        "2026-10-12T11:00:00+01:00",
    ]);
    let snapshot = tracker.json(&["snapshot", "--period", "day", "--date", "2026-10-12"]);
    assert_eq!(snapshot["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(
        snapshot["sessions"][0]["id"].as_i64().unwrap().to_string(),
        id
    );
    assert_eq!(
        snapshot["sessions"][0]["start"],
        "2026-10-12T08:30:00+01:00"
    );
    assert_eq!(snapshot["sessions"][0]["end"], "2026-10-12T11:00:00+01:00");
    assert_eq!(snapshot["total_seconds"], 9000.0);
}
