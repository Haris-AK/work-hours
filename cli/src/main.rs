use anyhow::{Context, Result, bail};
use clap::Parser;
use std::{path::PathBuf, process::Command};
use work_hours::*;

#[derive(Parser)]
#[command(about = "Local work timer, native GTK dashboard, and bar/CSV interface")]
struct Args {
    #[arg(default_value = "open", value_parser = ["open","start","pause","toggle","status","bar","summary","export","note","snapshot","session-save","session-remove","session-restore","idle-pause"])]
    command: String,
    #[arg(long)]
    text: Option<String>,
    /// Replace the selected day's note instead of appending.
    #[arg(long, requires = "text")]
    replace: bool,
    #[arg(long, requires = "replace")]
    expected_note: Option<String>,
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    end: Option<String>,
    #[arg(long)]
    id: Option<i64>,
    #[arg(long, default_value = "week", value_parser = ["day","week","month"])]
    period: String,
    #[arg(long)]
    date: Option<Day>,
    #[arg(long = "from")]
    from_date: Option<Day>,
    #[arg(long = "to")]
    to_date: Option<Day>,
    #[arg(long, default_value = "daily", value_parser = ["daily","sessions"])]
    kind: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    db: Option<PathBuf>,
    /// For idle-pause: seconds the idle daemon waited, so that idle time is not counted.
    #[arg(long, default_value_t = 0.0, value_parser = idle_seconds)]
    idle_for: f64,
}
fn idle_seconds(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(s) if s.is_finite() && s >= 0.0 => Ok(s),
        _ => Err("expected a non-negative number of seconds".into()),
    }
}
fn run() -> Result<()> {
    let args = Args::parse();
    let path = args.db.unwrap_or_else(default_db);
    if args.command == "open" {
        // Keep GTK's startup/linker overhead out of the once-per-second CLI.
        let exe = std::env::current_exe()?.with_file_name("work-hours-dashboard");
        let status = Command::new(exe)
            .arg("--db")
            .arg(path)
            .status()
            .context("Dashboard missing: install work-hours-dashboard next to work-hours (see the README)")?;
        if !status.success() {
            bail!("Dashboard exited with {status}");
        }
        return Ok(());
    }
    let store = Store::open(&path)?;
    let now = now();
    // Only the bar runs inside the graphical session, so only it can tell a logout happened.
    if let Some((away, end)) = store.catch_up(now, &Presence::current(args.command == "bar"))? {
        notify_away(away, end);
    }
    let date = args.date.unwrap_or_else(today);
    match args.command.as_str() {
        "start" | "pause" | "toggle" => println!(
            "{}",
            if store.control(&args.command, now)? {
                "Working"
            } else {
                "Paused"
            }
        ),
        "idle-pause" => {
            if store.pause_if_running(now, args.idle_for)? {
                let idle = if args.idle_for > 0.0 {
                    format!("No activity for {}", spoken(args.idle_for))
                } else {
                    "No activity".into()
                };
                println!("Paused: {}.", idle.to_lowercase());
                let body = format!(
                    "{idle}. Your timer is now paused. Start it again when you return to work."
                );
                if !notify("Work Hours paused", &body) {
                    eprintln!(
                        "work-hours: timer paused, but the desktop notification could not be delivered"
                    );
                }
            }
        }
        "note" => {
            if let Some(text) = args.text {
                if args.replace {
                    store.replace_note(date, &text, args.expected_note.as_deref())?;
                    println!("Note saved for {date}");
                } else {
                    store.append_note(date, &text)?;
                    println!("Note added for {date}");
                }
            } else {
                let text = store.note(date)?;
                println!(
                    "{}",
                    if text.is_empty() {
                        "No notes for this day."
                    } else {
                        &text
                    }
                );
            }
        }
        "session-save" => {
            store.save_session(
                parse_time(&args.start.context("session-save requires --start")?)?,
                parse_time(&args.end.context("session-save requires --end")?)?,
                args.id,
                now,
            )?;
            println!("Session saved.");
        }
        "session-remove" => {
            store.remove(args.id.context("session-remove requires --id")?)?;
            println!("Session removed.");
        }
        "session-restore" => {
            store.restore(args.id.context("session-restore requires --id")?)?;
            println!("Session restored.");
        }
        "bar" => println!("{}", store.bar(now)?),
        "status" => println!(
            "{}",
            store.bar(now)?["tooltip"]
                .as_str()
                .unwrap()
                .split("\nClick:")
                .next()
                .unwrap()
        ),
        _ => {
            let (begin, end) = period_bounds(&args.period, date);
            let begin = args.from_date.unwrap_or(begin);
            let end = args.to_date.map(next).unwrap_or(end);
            if end <= begin {
                bail!("End date must be on or after start date.");
            }
            if args.command == "snapshot" {
                let rows = store.daily(begin, end, now)?;
                let sessions = store.sessions(begin, end, now)?;
                println!(
                    "{}",
                    serde_json::json!({
                        "begin": begin.to_string(), "end": (end - chrono::Days::new(1)).to_string(),
                        "today": today().to_string(), "running": store.active()?.is_some(),
                        "total_seconds": rows.iter().map(|r|r.seconds).sum::<f64>() + 0.0,
                        "days": rows.iter().map(|r|serde_json::json!({"date":r.day.to_string(),"seconds":r.seconds,"note":r.note})).collect::<Vec<_>>(),
                        "sessions": sessions.iter().map(|s|serde_json::json!({"id":s.id,"start":local_stamp(s.start),"end":s.end.map(local_stamp),"seconds":(s.end.unwrap_or(now)-s.start).max(0.0)})).collect::<Vec<_>>()
                    })
                );
            } else if args.command == "export" {
                let target = args.output.context("export requires --output PATH")?;
                store.export(&target, begin, end, &args.kind, now)?;
                println!("{}", target.display());
            } else {
                let rows = store.daily(begin, end, now)?;
                for r in &rows {
                    println!("{}  {:>9}  {}", r.day, duration(r.seconds), r.note);
                }
                println!("Total: {}", duration(rows.iter().map(|r| r.seconds).sum()));
            }
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("work-hours: {e:#}");
        std::process::exit(1);
    }
}
