use anyhow::{Context, Result};
use chrono::Days;
use clap::Parser;
use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
use work_hours::*;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    db: Option<PathBuf>,
}
struct Ui {
    store: Store,
    window: gtk::ApplicationWindow,
    anchor: Cell<Day>,
    note_day: Cell<Day>,
    edit_id: Cell<Option<i64>>,
    removed_id: Cell<Option<i64>>,
    loading: Cell<bool>,
    toggle: gtk::Button,
    status: gtk::Label,
    period: gtk::DropDown,
    heading: gtk::Label,
    days: gtk::ListBox,
    day_rows: RefCell<Vec<(Day, gtk::Label)>>,
    note_heading: gtk::Label,
    notes: gtk::TextView,
    sessions: gtk::ListBox,
    session_rows: RefCell<Vec<Session>>,
    start: gtk::Entry,
    end: gtk::Entry,
    message: gtk::Label,
}
fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l
}
fn scroll(child: &impl IsA<gtk::Widget>, height: i32, expand: bool) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .min_content_height(height)
        .vexpand(expand)
        .build()
}
fn clear(box_: &gtk::ListBox) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}
fn action(ui: &Rc<Ui>, button: &gtk::Button, f: impl Fn(&Rc<Ui>) -> Result<()> + 'static) {
    let weak = Rc::downgrade(ui);
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(f(&ui));
        }
    });
}
impl Ui {
    fn report(&self, result: Result<()>) {
        if let Err(e) = result {
            self.message.set_text(&format!("{e:#}"));
        }
    }
    fn bounds(&self) -> (Day, Day) {
        period_bounds(
            ["day", "week", "month"][self.period.selected() as usize],
            self.anchor.get(),
        )
    }
    fn save_note(&self) -> Result<()> {
        if !self.loading.get() {
            let b = self.notes.buffer();
            self.store.set_note(
                self.note_day.get(),
                &b.text(&b.start_iter(), &b.end_iter(), true),
            )?;
        }
        Ok(())
    }
    fn load_note(&self) -> Result<()> {
        let text = self.store.note(self.note_day.get())?;
        self.loading.set(true);
        self.note_heading.set_text(&format!(
            "Notes for {} · saved automatically",
            self.note_day.get()
        ));
        self.notes.buffer().set_text(&text);
        self.loading.set(false);
        Ok(())
    }
    fn day_text(r: &Daily) -> String {
        format!(
            "{}    {}    {}",
            r.day.format("%a %d %b"),
            duration(r.seconds),
            r.note
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(85)
                .collect::<String>()
        )
    }
    fn tick(&self) -> Result<()> {
        let now = now();
        let bar = self.store.bar(now)?;
        self.status.set_text(
            &bar["tooltip"]
                .as_str()
                .unwrap()
                .split("\nClick:")
                .next()
                .unwrap()
                .replace('\n', "    "),
        );
        self.toggle.set_label(if bar["class"] == "running" {
            "Pause working"
        } else {
            "Start working"
        });
        let (begin, end) = self.bounds();
        let rows = self.store.daily(begin, end, now)?;
        self.heading.set_text(&format!(
            "{begin} — {}    Total: {}",
            end - Days::new(1),
            duration(rows.iter().map(|r| r.seconds).sum())
        ));
        for (r, (_, l)) in rows.iter().zip(self.day_rows.borrow().iter()) {
            l.set_text(&Self::day_text(r));
        }
        Ok(())
    }
    fn refresh(&self) -> Result<()> {
        let (begin, end) = self.bounds();
        if self.note_day.get() < begin || self.note_day.get() >= end {
            self.save_note()?;
            self.note_day.set(self.anchor.get());
            self.load_note()?;
        }
        self.loading.set(true);
        clear(&self.days);
        self.day_rows.borrow_mut().clear();
        let result = (|| -> Result<()> {
            for r in self.store.daily(begin, end, now())? {
                let l = label(&Self::day_text(&r));
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&l));
                self.days.append(&row);
                self.day_rows.borrow_mut().push((r.day, l));
                if r.day == self.note_day.get() {
                    self.days.select_row(Some(&row));
                }
            }
            Ok(())
        })();
        self.loading.set(false);
        result?;
        self.refresh_sessions()?;
        self.tick()
    }
    fn refresh_sessions(&self) -> Result<()> {
        self.loading.set(true);
        clear(&self.sessions);
        self.loading.set(false);
        self.edit_id.set(None);
        self.start.set_text("");
        self.end.set_text("");
        let rows = self
            .store
            .sessions(self.note_day.get(), next(self.note_day.get()), now())?;
        for s in &rows {
            self.sessions.append(&label(&format!(
                "#{}    {} → {}",
                s.id,
                local_stamp(s.start),
                s.end.map(local_stamp).unwrap_or_else(|| "Running".into())
            )));
        }
        *self.session_rows.borrow_mut() = rows;
        Ok(())
    }
    fn move_period(&self, direction: i32) -> Result<()> {
        let (begin, end) = self.bounds();
        let days = if self.period.selected() == 1 { 7 } else { 1 };
        self.anchor.set(if self.period.selected() == 2 {
            if direction > 0 {
                end
            } else {
                begin - Days::new(1)
            }
        } else if direction > 0 {
            self.anchor.get() + Days::new(days)
        } else {
            self.anchor.get() - Days::new(days)
        });
        self.refresh()
    }
    fn export(self: &Rc<Self>, kind: &'static str) -> Result<()> {
        let (begin, end) = self.bounds();
        let chooser = gtk::FileChooserNative::new(
            Some("Export work hours"),
            Some(&self.window),
            gtk::FileChooserAction::Save,
            Some("Export"),
            Some("Cancel"),
        );
        chooser.set_current_name(&format!(
            "work-hours-{kind}-{begin}-{}.csv",
            end - Days::new(1)
        ));
        let weak = Rc::downgrade(self);
        chooser.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let Some(ui) = weak.upgrade()
            {
                ui.report((|| -> Result<()> {
                    let path = dialog
                        .file()
                        .and_then(|f| f.path())
                        .context("Choose a local file for the export.")?;
                    ui.store.export(&path, begin, end, kind, now())?;
                    ui.message.set_text(&format!("Exported {}", path.display()));
                    Ok(())
                })());
            }
            dialog.destroy();
        });
        chooser.show();
        Ok(())
    }
}
fn build(app: &gtk::Application, path: &std::path::Path) -> Result<Rc<Ui>> {
    let store = Store::open(path)?;
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Work Hours")
        .default_width(850)
        .default_height(760)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(20);
    root.set_margin_bottom(20);
    root.set_margin_start(20);
    root.set_margin_end(20);
    window.set_child(Some(&root));
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let toggle = gtk::Button::with_label("Start working");
    let status = label("");
    top.append(&toggle);
    top.append(&status);
    root.append(&top);
    root.append(&label("Auto-pauses after 15 minutes of desktop inactivity. Resume manually when you return. Correct a session below if needed."));
    let nav = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let period = gtk::DropDown::from_strings(&["Day", "Week", "Month"]);
    period.set_selected(1);
    nav.append(&period);
    let prev = gtk::Button::with_label("‹ Previous");
    let today_button = gtk::Button::with_label("Today");
    let next_button = gtk::Button::with_label("Next ›");
    let export_days = gtk::Button::with_label("Export days CSV");
    let export_sessions = gtk::Button::with_label("Export sessions CSV");
    for b in [
        &prev,
        &today_button,
        &next_button,
        &export_days,
        &export_sessions,
    ] {
        nav.append(b);
    }
    root.append(&nav);
    let heading = label("");
    root.append(&heading);
    let days = gtk::ListBox::new();
    root.append(&scroll(&days, 140, true));
    let note_heading = label("");
    root.append(&note_heading);
    let notes = gtk::TextView::new();
    notes.set_wrap_mode(gtk::WrapMode::WordChar);
    root.append(&scroll(&notes, 100, false));
    root.append(&label("Sessions for selected day — click a finished session to correct it. Times include the UTC offset."));
    let sessions = gtk::ListBox::new();
    root.append(&scroll(&sessions, 90, false));
    let form = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let start = gtk::Entry::new();
    let end = gtk::Entry::new();
    start.set_placeholder_text(Some("Start: YYYY-MM-DD HH:MM"));
    end.set_placeholder_text(Some("End: YYYY-MM-DD HH:MM"));
    start.set_hexpand(true);
    end.set_hexpand(true);
    form.append(&start);
    form.append(&end);
    root.append(&form);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let new_session = gtk::Button::with_label("New session");
    let save = gtk::Button::with_label("Save session");
    let remove = gtk::Button::with_label("Remove selected");
    let undo = gtk::Button::with_label("Undo removal");
    for b in [&new_session, &save, &remove, &undo] {
        actions.append(b);
    }
    root.append(&actions);
    let message = label("");
    root.append(&message);
    let ui = Rc::new(Ui {
        store,
        window,
        anchor: Cell::new(today()),
        note_day: Cell::new(today()),
        edit_id: Cell::new(None),
        removed_id: Cell::new(None),
        loading: Cell::new(false),
        toggle,
        status,
        period,
        heading,
        days,
        day_rows: RefCell::new(Vec::new()),
        note_heading,
        notes,
        sessions,
        session_rows: RefCell::new(Vec::new()),
        start,
        end,
        message,
    });
    action(&ui, &ui.toggle, |ui| {
        ui.store.control("toggle", now())?;
        ui.refresh()
    });
    action(&ui, &prev, |ui| ui.move_period(-1));
    action(&ui, &next_button, |ui| ui.move_period(1));
    action(&ui, &today_button, |ui| {
        ui.save_note()?;
        ui.anchor.set(today());
        ui.note_day.set(today());
        ui.load_note()?;
        ui.refresh()
    });
    action(&ui, &export_days, |ui| ui.export("daily"));
    action(&ui, &export_sessions, |ui| ui.export("sessions"));
    action(&ui, &new_session, |ui| {
        ui.sessions.unselect_all();
        ui.edit_id.set(None);
        ui.start.set_text(&format!("{} 09:00", ui.note_day.get()));
        ui.end.set_text(&format!("{} 10:00", ui.note_day.get()));
        ui.message
            .set_text("New session. Adjust both times, then save.");
        Ok(())
    });
    action(&ui, &save, |ui| {
        ui.store.save_session(
            parse_time(&ui.start.text())?,
            parse_time(&ui.end.text())?,
            ui.edit_id.get(),
            now(),
        )?;
        ui.refresh()?;
        ui.message.set_text("Session saved.");
        Ok(())
    });
    action(&ui, &remove, |ui| {
        if let Some(id) = ui.edit_id.get() {
            ui.store.remove(id)?;
            ui.removed_id.set(Some(id));
            ui.refresh()?;
            ui.message
                .set_text("Session removed. Use Undo removal to restore it.");
        }
        Ok(())
    });
    action(&ui, &undo, |ui| {
        if let Some(id) = ui.removed_id.get() {
            ui.store.restore(id)?;
            ui.removed_id.set(None);
            ui.refresh()?;
            ui.message.set_text("Session restored.");
        }
        Ok(())
    });
    let weak = Rc::downgrade(&ui);
    ui.period.connect_selected_notify(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(ui.refresh());
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.notes.buffer().connect_changed(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(ui.save_note());
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.days.connect_row_selected(move |_, row| {
        if let Some(ui) = weak.upgrade()
            && !ui.loading.get()
            && let Some(row) = row
        {
            let day = ui.day_rows.borrow().get(row.index() as usize).map(|r| r.0);
            if let Some(day) = day {
                ui.report((|| {
                    ui.save_note()?;
                    ui.note_day.set(day);
                    ui.load_note()?;
                    ui.refresh_sessions()
                })());
            }
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.sessions.connect_row_selected(move |_, row| {
        if let Some(ui) = weak.upgrade()
            && !ui.loading.get()
            && let Some(row) = row
            && let Some(s) = ui.session_rows.borrow().get(row.index() as usize)
        {
            if let Some(end) = s.end {
                ui.edit_id.set(Some(s.id));
                ui.start.set_text(&local_stamp(s.start));
                ui.end.set_text(&local_stamp(end));
                ui.message.set_text(&format!("Editing session #{}", s.id));
            } else {
                ui.edit_id.set(None);
                ui.start.set_text("");
                ui.end.set_text("");
                ui.message
                    .set_text("Pause this session before correcting it.");
            }
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.window.connect_close_request(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.report(ui.save_note());
        }
        glib::Propagation::Proceed
    });
    ui.load_note()?;
    ui.refresh()?;
    ui.window.present();
    // The timer owns the UI until the window closes; callbacks hold weak references.
    let ticking = Rc::clone(&ui);
    glib::timeout_add_local(Duration::from_secs(1), move || {
        let ui = &ticking;
        if !ui.window.is_visible() {
            return glib::ControlFlow::Break;
        }
        ui.report(ui.tick());
        glib::ControlFlow::Continue
    });
    Ok(ui)
}
fn main() -> glib::ExitCode {
    let args = Args::parse();
    let path = args.db.unwrap_or_else(default_db);
    let app = gtk::Application::builder()
        .application_id("local.workhours.Tracker")
        .build();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else if let Err(e) = build(app, &path) {
            eprintln!("work-hours: {e:#}");
            app.quit();
        }
    });
    app.run_with_args::<&str>(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn click(ui: &Ui, text: &str) {
        fn find(widget: gtk::Widget, text: &str) -> Option<gtk::Button> {
            if let Ok(b) = widget.clone().downcast::<gtk::Button>()
                && b.label().as_deref() == Some(text)
            {
                return Some(b);
            }
            let mut child = widget.first_child();
            while let Some(w) = child {
                if let Some(b) = find(w.clone(), text) {
                    return Some(b);
                }
                child = w.next_sibling();
            }
            None
        }
        find(ui.window.clone().upcast(), text)
            .expect("button missing")
            .emit_clicked();
    }
    #[test]
    fn native_dashboard_controls_notes_and_corrections() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("local.workhours.Test")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let ui = build(&app, &dir.path().join("db")).unwrap();
        ui.toggle.emit_clicked();
        assert!(ui.store.active().unwrap().is_some());
        ui.toggle.emit_clicked();
        assert!(ui.store.active().unwrap().is_none());
        ui.period.set_selected(0);
        let day = today() - Days::new(2);
        ui.anchor.set(day);
        ui.refresh().unwrap();
        ui.notes.buffer().set_text("Dashboard note");
        assert_eq!(ui.store.note(day).unwrap(), "Dashboard note");
        click(&ui, "New session");
        click(&ui, "Save session");
        assert_eq!(ui.message.text(), "Session saved.");
        assert_eq!(
            ui.store.daily(day, next(day), now()).unwrap()[0].seconds,
            3600.0
        );
        ui.sessions.select_row(ui.sessions.row_at_index(0).as_ref());
        ui.end.set_text(&format!("{day} 11:00"));
        click(&ui, "Save session");
        assert_eq!(
            ui.store.daily(day, next(day), now()).unwrap()[0].seconds,
            7200.0
        );
        ui.sessions.select_row(ui.sessions.row_at_index(0).as_ref());
        click(&ui, "Remove selected");
        assert_eq!(
            ui.store.daily(day, next(day), now()).unwrap()[0].seconds,
            0.0
        );
        click(&ui, "Undo removal");
        assert_eq!(
            ui.store.daily(day, next(day), now()).unwrap()[0].seconds,
            7200.0
        );
        click(&ui, "Next ›");
        ui.notes.buffer().set_text("Other day");
        click(&ui, "‹ Previous");
        assert_eq!(
            ui.notes.buffer().text(
                &ui.notes.buffer().start_iter(),
                &ui.notes.buffer().end_iter(),
                true
            ),
            "Dashboard note"
        );
        ui.tick().unwrap();
        ui.window.close();
    }
}
