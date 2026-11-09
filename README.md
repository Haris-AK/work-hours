# Work Hours

A local work timer written in Rust for Hyprland, Wayle, Waybar, and Vicinae. Sessions and notes
are stored in SQLite at `$XDG_DATA_HOME/work-hours/work-hours.sqlite3`
(default `~/.local/share/work-hours/work-hours.sqlite3`). No account or server
is required. Back up that database while the tracker is closed/paused.

## Desktop controls

- Bar: left-click the hours to start/pause; right-click to open the dashboard.
- Hyprland: **Super+Ctrl+T** toggles; **Super+Shift+T** opens the dashboard.
- Vicinae: search **Work Hours — Manage Timer and History**.

The bar shows today's accumulated hours and an explicit **Working** or **Paused**
label. Wayle uses green for Working and amber for Paused.
Hover for today's, this week's, and this month's totals.
The dashboard offers day/week/month navigation, daily notes that save as you
type, and individual sessions. Select a finished session to correct its times,
or choose **New session** to add missing time. Overlapping sessions are rejected.
**Remove selected** has an **Undo removal** action; deleted records remain in
the database. Undo applies to the latest removal during that dashboard visit.

On Hyprland, **15 minutes without keyboard or mouse activity automatically pauses
an active timer** and sends a **Work Hours paused** desktop notification. The
15-minute grace period is included in the session. Already-paused timers do not
produce notifications. Returning to the computer does not resume the timer;
start it again when you return to work. This rule ignores media/presentation idle
inhibitors. It is configured in `.config/hypr/hypridle.conf`, and Hypridle must
be running for it to work.

The timer counts elapsed wall-clock time until paused, **including sleep,
logout, and shutdown**. Idle detection runs while the desktop is awake; it cannot
send a notification while the computer is suspended or off. It survives restarting the bar or dashboard. Pause for
breaks/end of work; correct the end time if you forget. It does not automatically
infer whether you are working. No daemon is needed.

Weeks begin Monday. Months are calendar months. Summaries use the computer's
current local timezone and actual elapsed seconds across daylight-saving changes.
Sessions crossing midnight contribute time to each day. Changing your timezone
will regroup historical hours into the new local dates. Notes remain tied to
the date on which they were written. Displays show hours, minutes, and seconds and refresh every second; exports
retain seconds and decimal hours.

## Vicinae

Search **Work Hours — Manage Timer and History** for the interactive extension:

- Select the timer and press **Enter** to start/pause, or use **Ctrl+T**.
- Select a day and press **Enter** to open its notes and sessions.
- Press **Ctrl+B** for Vicinae's action menu (the default binding). Actions include
  **Edit Daily Note** (**Ctrl+N**), **Add Session**, previous/next period, go to a
  date, and daily/session CSV export. Choose Day/Week/Month from the dropdown.
- Select a finished session to edit its times or remove it. The day's view includes
  **Undo Last Removal**. Pause an active session before editing it.
- Notes use an explicit **Save Note** action. Saving an empty note clears it.
  If the note changed elsewhere while the form was open, saving is rejected;
  reopen the editor to load the latest note.
- Exports go to `~/Documents/Work Hours/` (or `$XDG_DOCUMENTS_DIR/Work Hours/`)
  and show actions to copy the path or reveal the file.

The extension renders inside Vicinae and calls the same Rust tracker/database as
Wayle and the GTK dashboard. Its source is in `vicinae/`. No separate
storage or timer is used. Totals refresh every second while the extension is open.

The old one-shot script commands have been removed in favour of this single
interactive entry. The GTK dashboard's desktop entry is hidden from app search
to avoid another duplicate; it still opens from the bar, keyboard shortcut, or
`work-hours open`.

## Export and terminal use

Both CSV formats can be opened in a spreadsheet. Daily exports include the date,
seconds, decimal hours, and notes, including days with no recorded time.
Session exports include session ID, start/end with UTC offsets, seconds, hours,
running status, date, and that day's notes. They split sessions at midnight and
clip them to the selected period, so totals reconcile with daily exports. Active
sessions are exported up to the export time and marked as running. Notes repeated
across session rows belong to the day, not to individual sessions.

```sh
work-hours start
work-hours pause
work-hours status
work-hours note --text 'Reviewed PRs and fixed the import bug'
work-hours summary --period week --date 2026-10-14
work-hours export --period month --kind daily --output ~/hours.csv
work-hours export --from 2026-10-01 --to 2026-10-14 --kind sessions --output ~/sessions.csv
```

`--to` is inclusive. Exports refuse to overwrite an existing file. The dashboard
exports the currently displayed period and lets you choose a file destination.

## Installation and validation

Requires Rust/Cargo, `pkg-config`, GTK 4 and SQLite development libraries to build
(on Arch/CachyOS: `rust`, `pkgconf`, `gtk4`, `sqlite`). The installed app does not
require Python. Hyprland idle auto-pause also uses `hypridle` and `notify-send`
(`libnotify` on Arch/CachyOS). Run `./install.sh` from this repository to build
the release binaries and link the executable and desktop entry.
Re-run it after changing the Rust source in `cli/`.
When Vicinae and npm are available, it also builds the interactive extension.
To rebuild only the extension, run `./install_vicinae.sh`.
Building the extension requires Node.js 20+ and npm. If installing your first local
extension, restart Vicinae once with `vicinae server --replace` if it does not appear.

The CLI and GTK dashboard are separate Rust binaries, so the once-per-second bar
command does not load GTK. The installer copies both binaries to `~/.local/lib/work-hours/`
and links `~/.local/bin/work-hours` to the CLI there. Existing SQLite sessions and notes are
reused directly; no conversion or reset is needed. The script refuses to replace unrelated files.
The bar configurations, Hyprland shortcuts and hypridle listener live in the
dotfiles repository and call `~/.local/bin/work-hours`. Wayle may need
`wayle panel restart` after its configuration changes. Restart Hypridle after
installing its configuration to activate the 15-minute idle listener.

```sh
cargo test --manifest-path cli/Cargo.toml
# Native UI check (requires a graphical session):
cargo test --manifest-path cli/Cargo.toml --features dashboard --bin work-hours-dashboard -- --test-threads=1
cargo clippy --manifest-path cli/Cargo.toml --features dashboard --all-targets -- -D warnings
# Native Rust CLI tests are included in cargo test above.
# Fresh-process bar benchmark (release build, isolated database):
cargo bench --manifest-path cli/Cargo.toml --bench bar_poll
npm run --prefix vicinae typecheck
npm run --prefix vicinae test
```

For experiments, pass `--db /tmp/work-hours-test.sqlite3` to use a separate database.
The Rust benchmark defaults to a 10 ms median polling budget; set
`WORK_HOURS_MAX_POLL_MS` to change it or `WORK_HOURS_BINARY` to benchmark another
build. No Python is needed for the tests or benchmark.
