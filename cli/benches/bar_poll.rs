//! Fresh-process bar latency, measured separately from correctness tests.
use std::{path::PathBuf, process::Command, time::Instant};

fn main() {
    let directory = tempfile::tempdir().expect("create isolated benchmark directory");
    let db = directory.path().join("work-hours.sqlite3");
    let binary = std::env::var_os("WORK_HOURS_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_work-hours")));
    let budget: f64 = std::env::var("WORK_HOURS_MAX_POLL_MS")
        .unwrap_or_else(|_| "10".into())
        .parse()
        .expect("WORK_HOURS_MAX_POLL_MS must be a positive number");
    assert!(
        budget.is_finite() && budget > 0.0,
        "Budget must be a positive finite number"
    );
    let poll = || {
        let output = Command::new(&binary)
            .arg("bar")
            .arg("--db")
            .arg(&db)
            .output()
            .expect("run work-hours bar");
        assert!(
            output.status.success(),
            "Bar command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bar: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("valid bar JSON");
        assert!(bar["text"].is_string());
    };
    // Initialize the schema and warm filesystem caches outside the timed samples.
    poll();
    let mut samples: Vec<_> = (0..30)
        .map(|_| {
            let start = Instant::now();
            poll();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    let median = (samples[14] + samples[15]) / 2.0;
    println!(
        "Bar poll: median {median:.2} ms; max {:.2} ms (30 fresh processes); budget {budget:.2} ms",
        samples[29]
    );
    assert!(median <= budget, "Bar polling exceeded its latency budget");
}
