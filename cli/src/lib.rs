use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

pub type Day = NaiveDate;
#[derive(Clone, Debug)]
pub struct Session {
    pub id: i64,
    pub start: f64,
    pub end: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Daily {
    pub day: Day,
    pub seconds: f64,
    pub note: String,
}
pub struct Store {
    pub db: Connection,
}

pub fn now() -> f64 {
    Utc::now().timestamp_micros() as f64 / 1_000_000.0
}
pub fn today() -> Day {
    Local::now().date_naive()
}
pub fn next(day: Day) -> Day {
    day.checked_add_days(Days::new(1)).expect("date overflow")
}
pub fn local_time(stamp: f64) -> DateTime<Local> {
    DateTime::from_timestamp_micros((stamp * 1_000_000.0).round() as i64)
        .expect("timestamp out of range")
        .with_timezone(&Local)
}
pub fn local_stamp(stamp: f64) -> String {
    local_time(stamp).to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}
pub fn midnight(day: Day) -> Result<f64> {
    let value = Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).context("invalid midnight")?)
        .earliest()
        .context("Midnight does not exist in this timezone")?;
    Ok(value.timestamp() as f64)
}
pub fn parse_time(text: &str) -> Result<f64> {
    if let Ok(t) = DateTime::parse_from_rfc3339(text) {
        return Ok(t.timestamp_micros() as f64 / 1_000_000.0);
    }
    let naive = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ]
    .iter()
    .find_map(|f| NaiveDateTime::parse_from_str(text, f).ok())
    .context("Use YYYY-MM-DD HH:MM or an ISO time with UTC offset")?;
    let t = Local
        .from_local_datetime(&naive)
        .single()
        .context("Ambiguous or nonexistent local time; include a UTC offset")?;
    Ok(t.timestamp() as f64)
}
pub fn duration(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}h {:02}m {:02}s", s / 3600, s / 60 % 60, s % 60)
}
pub fn period_bounds(kind: &str, anchor: Day) -> (Day, Day) {
    match kind {
        "day" => (anchor, next(anchor)),
        "week" => {
            let start = anchor - Days::new(anchor.weekday().num_days_from_monday() as u64);
            (start, start + Days::new(7))
        }
        _ => {
            let start = anchor.with_day(1).unwrap();
            let end = if anchor.month() == 12 {
                Day::from_ymd_opt(anchor.year() + 1, 1, 1)
            } else {
                Day::from_ymd_opt(anchor.year(), anchor.month() + 1, 1)
            }
            .unwrap();
            (start, end)
        }
    }
}
pub fn default_db() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("work-hours/work-hours.sqlite3")
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(10))?;
        // Preserve the original Python schema and timestamps. DDL only runs on first use.
        if db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))? == 0 {
            db.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE IF NOT EXISTS sessions (id INTEGER PRIMARY KEY, start REAL NOT NULL, end REAL, deleted INTEGER NOT NULL DEFAULT 0, CHECK(end IS NULL OR end >= start));
                CREATE UNIQUE INDEX IF NOT EXISTS one_running ON sessions((1)) WHERE end IS NULL AND deleted=0;
                CREATE INDEX IF NOT EXISTS session_start ON sessions(start) WHERE deleted=0;
                CREATE TABLE IF NOT EXISTS notes (day TEXT PRIMARY KEY, text TEXT NOT NULL);
                PRAGMA user_version=1; COMMIT;")?;
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(Self { db })
    }
    fn transaction(&self) -> Result<Transaction<'_>> {
        Ok(Transaction::new_unchecked(
            &self.db,
            TransactionBehavior::Immediate,
        )?)
    }
    pub fn active(&self) -> Result<Option<Session>> {
        Ok(self
            .db
            .query_row(
                "SELECT id,start,end FROM sessions WHERE end IS NULL AND deleted=0",
                [],
                session_row,
            )
            .optional()?)
    }
    pub fn control(&self, action: &str, now: f64) -> Result<bool> {
        let tx = self.transaction()?;
        let active = self.active()?;
        let running = match active {
            Some(s) if action == "pause" || action == "toggle" => {
                tx.execute(
                    "UPDATE sessions SET end=? WHERE id=?",
                    params![now.max(s.start), s.id],
                )?;
                false
            }
            None if action == "start" || action == "toggle" => {
                tx.execute("INSERT INTO sessions(start) VALUES (?)", [now])?;
                true
            }
            other => other.is_some(),
        };
        tx.commit()?;
        Ok(running)
    }
    /// Atomically pause only a running timer. Repeated idle events are silent.
    pub fn pause_if_running(&self, now: f64) -> Result<bool> {
        Ok(self.db.execute(
            "UPDATE sessions SET end=MAX(start,?) WHERE end IS NULL AND deleted=0",
            [now],
        )? > 0)
    }
    pub fn save_session(&self, start: f64, end: f64, id: Option<i64>, now: f64) -> Result<()> {
        if !start.is_finite() || !end.is_finite() || end <= start {
            bail!("End must be after start.");
        }
        if end > now {
            bail!("Completed sessions cannot end in the future.");
        }
        let tx = self.transaction()?;
        if let Some(id) = id {
            let editable: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=? AND deleted=0 AND end IS NOT NULL)", [id], |r| r.get(0))?;
            if !editable {
                bail!("Pause the timer before editing its session.");
            }
        }
        self.check_overlap(start, end, id.unwrap_or(-1))?;
        match id {
            Some(id) => {
                tx.execute(
                    "UPDATE sessions SET start=?,end=? WHERE id=?",
                    params![start, end, id],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO sessions(start,end) VALUES (?,?)",
                    params![start, end],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    fn check_overlap(&self, start: f64, end: f64, id: i64) -> Result<()> {
        let overlap: Option<i64> = self.db.query_row("SELECT id FROM sessions WHERE deleted=0 AND id != ? AND start < ? AND (end IS NULL OR end > ?) LIMIT 1", params![id,end,start], |r| r.get(0)).optional()?;
        if let Some(id) = overlap {
            bail!("This overlaps session #{id}.");
        }
        Ok(())
    }
    pub fn remove(&self, id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE sessions SET deleted=1 WHERE id=? AND end IS NOT NULL",
            [id],
        )?;
        Ok(())
    }
    pub fn restore(&self, id: i64) -> Result<()> {
        let tx = self.transaction()?;
        let row = tx
            .query_row(
                "SELECT id,start,end FROM sessions WHERE id=? AND deleted=1",
                [id],
                session_row,
            )
            .optional()?;
        if let Some(s) = row {
            self.check_overlap(
                s.start,
                s.end.context("Cannot restore an active session")?,
                id,
            )?;
            tx.execute("UPDATE sessions SET deleted=0 WHERE id=?", [id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn note(&self, day: Day) -> Result<String> {
        Ok(self
            .db
            .query_row(
                "SELECT text FROM notes WHERE day=?",
                [day.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }
    pub fn set_note(&self, day: Day, text: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO notes VALUES (?,?) ON CONFLICT(day) DO UPDATE SET text=excluded.text",
            params![day.to_string(), text],
        )?;
        Ok(())
    }
    pub fn replace_note(&self, day: Day, text: &str, expected: Option<&str>) -> Result<()> {
        let tx = self.transaction()?;
        if let Some(expected) = expected
            && self.note(day)? != expected
        {
            bail!(
                "This note changed elsewhere. Reopen the note editor to load the latest version."
            );
        }
        self.set_note(day, text)?;
        tx.commit()?;
        Ok(())
    }
    pub fn append_note(&self, day: Day, text: &str) -> Result<()> {
        let tx = self.transaction()?;
        let old = self.note(day)?;
        self.set_note(
            day,
            &format!("{old}{}{text}", if old.is_empty() { "" } else { "\n" }),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn sessions(&self, start: Day, end: Day, now: f64) -> Result<Vec<Session>> {
        let mut stmt = self.db.prepare_cached("SELECT id,start,end FROM sessions WHERE deleted=0 AND start < ? AND COALESCE(end,?) > ? ORDER BY start")?;
        Ok(stmt
            .query_map(params![midnight(end)?, now, midnight(start)?], session_row)?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn daily(&self, start: Day, end: Day, now: f64) -> Result<Vec<Daily>> {
        let sessions = self.sessions(start, end, now)?;
        let mut stmt = self
            .db
            .prepare_cached("SELECT day,text FROM notes WHERE day >= ? AND day < ?")?;
        let notes: HashMap<String, String> = stmt
            .query_map(params![start.to_string(), end.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut rows = Vec::new();
        let mut day = start;
        while day < end {
            let (lower, upper) = (midnight(day)?, midnight(next(day))?);
            let seconds = sessions
                .iter()
                .map(|s| (s.end.unwrap_or(now).min(upper) - s.start.max(lower)).max(0.0))
                .sum();
            rows.push(Daily {
                day,
                seconds,
                note: notes.get(&day.to_string()).cloned().unwrap_or_default(),
            });
            day = next(day);
        }
        Ok(rows)
    }
    pub fn bar(&self, now: f64) -> Result<serde_json::Value> {
        let today = local_time(now).date_naive();
        let (week, week_end) = period_bounds("week", today);
        let (month, month_end) = period_bounds("month", today);
        let rows = self.daily(week.min(month), week_end.max(month_end), now)?;
        let total = |start: Day, end: Day| {
            rows.iter()
                .filter(|r| r.day >= start && r.day < end)
                .map(|r| r.seconds)
                .sum::<f64>()
        };
        let running = self.active()?.is_some();
        let day_total = total(today, next(today));
        Ok(
            serde_json::json!({"text": format!("{} {} · {}", if running {"▶"} else {"Ⅱ"}, if running {"Working"} else {"Paused"}, duration(day_total)), "class": if running {"running"} else {"paused"}, "tooltip": format!("{}\nToday: {}\nWeek: {}\nMonth: {}\nAuto-pause: after 15 minutes idle · Resume manually\nClick: start/pause · Right-click: history and notes", if running {"Working"} else {"Paused"}, duration(day_total), duration(total(week,week_end)), duration(total(month,month_end)))}),
        )
    }
    pub fn export(&self, target: &Path, start: Day, end: Day, kind: &str, now: f64) -> Result<()> {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let mut writer = csv::Writer::from_writer(file);
        let rounded = |s: f64| format!("{:.3}", s);
        if kind == "daily" {
            writer.write_record(["date", "seconds", "hours", "notes"])?;
            for r in self.daily(start, end, now)? {
                writer.write_record([
                    r.day.to_string(),
                    rounded(r.seconds),
                    format!("{:.6}", r.seconds / 3600.0),
                    r.note,
                ])?;
            }
        } else {
            writer.write_record([
                "session_id",
                "start",
                "end",
                "seconds",
                "hours",
                "running",
                "date",
                "notes",
            ])?;
            for s in self.sessions(start, end, now)? {
                let mut lower = s.start.max(midnight(start)?);
                let upper = s.end.unwrap_or(now).min(midnight(end)?);
                while lower < upper {
                    let day = local_time(lower).date_naive();
                    let stop = upper.min(midnight(next(day))?);
                    let seconds = stop - lower;
                    writer.write_record([
                        s.id.to_string(),
                        local_stamp(lower),
                        local_stamp(stop),
                        rounded(seconds),
                        format!("{:.6}", seconds / 3600.0),
                        if s.end.is_none() { "True" } else { "False" }.into(),
                        day.to_string(),
                        self.note(day)?,
                    ])?;
                    lower = stop;
                }
            }
        }
        writer.flush()?;
        Ok(())
    }
}
fn session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        start: r.get(1)?,
        end: r.get(2)?,
    })
}

#[cfg(test)]
mod tests;
