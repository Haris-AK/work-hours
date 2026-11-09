# Work Hours

A local work timer for Linux desktops, written in Rust. Start and pause it from
your bar, a keyboard shortcut, [Vicinae](https://github.com/vicinaehq/vicinae)
or the terminal. Add daily notes, correct sessions, and export timesheets as CSV.

Everything is stored in one SQLite database at
`$XDG_DATA_HOME/work-hours/work-hours.sqlite3`
(default `~/.local/share/work-hours/work-hours.sqlite3`). There is no account,
server or background daemon.

- **Bar module**: today's hours with a **Working**/**Paused** label. Hover to
  see today's, this week's and this month's totals.
- **GTK dashboard**: day/week/month views, daily notes, and session editing.
- **Vicinae extension**: the same features inside the launcher.
- **Away detection**: idle time, sleep, shutdown and logout are not counted.

## Installation

Build requirements: Rust/Cargo, `pkg-config`, and GTK 4 and SQLite development
libraries (Arch: `rust pkgconf gtk4 sqlite`). Notifications use `notify-send`
(Arch: `libnotify`).

```sh
cargo install --locked --path cli --features dashboard --root ~/.local
```

This installs `work-hours` and its dashboard into `~/.local/bin`. Run it again
to update; the database is upgraded in place.

**Vicinae extension** (optional, Node.js 20+):

```sh
cd vicinae && npm ci && npm run build
```

If the extension does not appear the first time, run `vicinae server --replace`.

## Desktop setup

The bar module, shortcuts and idle listener are configured in your own
desktop config. These examples are for Waybar and Hyprland.

**Waybar** (`work-hours bar` prints JSON with `text`, `tooltip`, and a
`running`/`paused` class for styling):

```jsonc
"custom/work-hours": {
    "exec": "$HOME/.local/bin/work-hours bar",
    "return-type": "json",
    "interval": 1,
    "on-click": "$HOME/.local/bin/work-hours toggle",
    "on-click-right": "$HOME/.local/bin/work-hours open"
}
```

**Hyprland shortcuts**:

```ini
bind = SUPER CTRL, T, exec, ~/.local/bin/work-hours toggle
bind = SUPER SHIFT, T, exec, ~/.local/bin/work-hours open
```

**Idle auto-pause** (`~/.config/hypr/hypridle.conf`, then restart Hypridle):

```ini
# Media playback idle inhibitors must not keep the timer running.
listener {
    timeout = 900
    on-timeout = ~/.local/bin/work-hours idle-pause --idle-for 900
    ignore_inhibit = true
}
```

`idle-pause --idle-for SECONDS` pauses a running timer and backdates the end
to when you went idle. It also sends a notification. Use the same value as
the idle timeout. Any idle tool works, e.g.
`swayidle timeout 900 'work-hours idle-pause --idle-for 900'`.

## How time is counted

The timer runs until you pause it. Returning to the computer never resumes it
automatically. These events also pause it, each with a notification that says
when the timer stopped:

| Event | Session ends at |
|---|---|
| Idle (via `idle-pause`) | When you went idle |
| Suspend or hibernate of 1 minute or more | When the computer went to sleep |
| Shutdown or restart | When the computer was last seen (within ~10 s if the bar is running) |
| Logout | When the bar was last seen in the old login session |

Detection needs no daemon. The bar, dashboard and CLI compare the running
session against the last observed boot ID, the kernel's suspended time and the
login session. If the bar is simply not running, nothing is cut. Without
the bar, a shutdown ends the session at the last `work-hours` command, so you
may need to correct the end time.

Weeks start on Monday and months are calendar months. Totals use the current
local timezone and real elapsed seconds, including across daylight-saving changes.
Sessions that cross midnight count toward both days. If you change timezone,
past hours are regrouped by the new local dates. Notes stay on the date
they were written for.

## Dashboard

Open it with right-click on the bar, a shortcut, or `work-hours open`.

- Browse by day, week or month. Notes save as you type.
- Select a finished session to correct its times, or choose **New session** to
  add missed time. Overlapping sessions are rejected.
- **Remove selected** can be undone with **Undo removal** (latest removal,
  while the dashboard stays open). Removed sessions stay in the database.
- Export the displayed period as CSV.

## Vicinae

Search for **Work Hours — Manage Timer and History**.

- On the timer, press **Enter** or **Ctrl+T** to start/pause. On a day, press
  **Enter** to open its notes and sessions.
- **Ctrl+B** opens the action menu: **Edit Daily Note** (**Ctrl+N**), **Add
  Session**, previous/next period, go to date, and CSV export. Use the dropdown
  to pick Day/Week/Month.
- Select a finished session to edit or remove it. **Undo Last Removal** is in
  the day view. Pause a running session before editing it.
- Notes save with **Save Note**; saving an empty note clears it. A save is
  rejected if the note changed elsewhere in the meantime.
- Exports go to `$XDG_DOCUMENTS_DIR/Work Hours/` (default
  `~/Documents/Work Hours/`).

The extension (in `vicinae/`) calls the same `work-hours` binary and database.

## Command line

```sh
work-hours start | pause | toggle | status
work-hours note --text 'Reviewed PRs and fixed the import bug'
work-hours summary --period week --date 2026-10-14
work-hours export --period month --kind daily --output ~/hours.csv
work-hours export --from 2026-10-01 --to 2026-10-14 --kind sessions --output ~/sessions.csv
```

Pass `--db PATH` to use a different database, e.g. for experiments.

**CSV exports.** `--to` is inclusive, and existing files are never overwritten.
- **Daily** exports have one row per day (including empty days) with seconds,
  decimal hours and notes.
- **Session** exports list each session with its start/end (with UTC
  offset), seconds, hours, a running flag, date, and that day's notes. Sessions
  are split at midnight and clipped to the period, so totals match the daily
  export. A running session is exported up to the current time.

## Development

The CLI and GTK dashboard are separate binaries, so the once-per-second bar
poll never loads GTK.

```sh
cargo test --manifest-path cli/Cargo.toml
cargo clippy --manifest-path cli/Cargo.toml --features dashboard --all-targets -- -D warnings
# Dashboard UI test (needs a graphical session):
cargo test --manifest-path cli/Cargo.toml --features dashboard --bin work-hours-dashboard -- --test-threads=1
# Bar poll latency, fresh processes on an isolated database:
cargo bench --manifest-path cli/Cargo.toml --bench bar_poll
npm run --prefix vicinae typecheck
npm run --prefix vicinae test
```

The benchmark fails if the median poll exceeds 10 ms. Set `WORK_HOURS_MAX_POLL_MS`
to change the budget, or `WORK_HOURS_BINARY` to benchmark another build.
