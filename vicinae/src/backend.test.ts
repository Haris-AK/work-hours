import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import test from "node:test";
import { createBackend, duration, moveDate, validDate } from "./backend";

const binary = resolve("../cli/target/release/work-hours");
async function fixture() {
  const directory = await mkdtemp(join(tmpdir(), "work-hours-vicinae-"));
  return {
    directory,
    backend: createBackend({
      binary,
      db: join(directory, "db"),
      exportDirectory: join(directory, "exports"),
    }),
    close: () => rm(directory, { recursive: true, force: true }),
  };
}
test("timer actions update the shared snapshot", async () => {
  const f = await fixture();
  try {
    assert.equal(
      (await f.backend.snapshot("day", "2026-10-12")).running,
      false,
    );
    await f.backend.control("start");
    assert.equal((await f.backend.snapshot("day", "2026-10-12")).running, true);
    await f.backend.control("pause");
    assert.equal(
      (await f.backend.snapshot("day", "2026-10-12")).running,
      false,
    );
  } finally {
    await f.close();
  }
});
test("note editing preserves multiline text and detects conflicting edits", async () => {
  const f = await fixture();
  try {
    const text = '--Flag, "quotes"\n$(printf unsafe); second line';
    await f.backend.saveNote("2026-10-12", text, "");
    assert.equal(
      (await f.backend.snapshot("day", "2026-10-12")).days[0].note,
      text,
    );
    await assert.rejects(
      f.backend.saveNote("2026-10-12", "Stale edit", ""),
      /changed elsewhere/,
    );
    await f.backend.saveNote("2026-10-12", "", text);
    assert.equal(
      (await f.backend.snapshot("day", "2026-10-12")).days[0].note,
      "",
    );
  } finally {
    await f.close();
  }
});
test("session forms, overlap rejection, removal, undo, and CSV export", async () => {
  const f = await fixture();
  try {
    await f.backend.saveSession(
      "2026-10-12T23:30:00+01:00",
      "2026-10-13T00:30:00+01:00",
    );
    let snapshot = await f.backend.snapshot("week", "2026-10-12");
    assert.equal(snapshot.total_seconds, 3600);
    const id = snapshot.sessions[0].id;
    await assert.rejects(
      f.backend.saveSession(
        "2026-10-12T23:45:00+01:00",
        "2026-10-13T01:00:00+01:00",
      ),
      /overlaps/,
    );
    await f.backend.saveSession(
      "2026-10-12T23:30:00+01:00",
      "2026-10-13T01:00:00+01:00",
      id,
    );
    await f.backend.removeSession(id);
    assert.equal(
      (await f.backend.snapshot("week", "2026-10-12")).total_seconds,
      0,
    );
    await f.backend.restoreSession(id);
    snapshot = await f.backend.snapshot("week", "2026-10-12");
    assert.equal(snapshot.total_seconds, 5400);
    for (const kind of ["daily", "sessions"] as const) {
      const path = await f.backend.exportCsv(snapshot, kind);
      const csv = await readFile(path, "utf8");
      assert.ok(csv.includes("2026-10-12"));
      assert.ok(csv.includes("2026-10-13"));
      const another = await f.backend.exportCsv(snapshot, kind);
      assert.notEqual(path, another);
    }
  } finally {
    await f.close();
  }
});
test("calendar input and second formatting", () => {
  assert.equal(validDate("2024-02-29"), true);
  assert.equal(validDate("2025-02-29"), false);
  assert.equal(validDate("2026-02-31"), false);
  assert.equal(moveDate("2026-12-31", 1), "2027-01-01");
  assert.equal(duration(5025), "1h 23m 45s");
});
