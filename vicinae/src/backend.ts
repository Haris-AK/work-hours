import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

export type Period = "day" | "week" | "month";
export interface Day {
  date: string;
  seconds: number;
  note: string;
}
export interface Session {
  id: number;
  start: string;
  end: string | null;
  seconds: number;
}
export interface Snapshot {
  begin: string;
  end: string;
  today: string;
  running: boolean;
  total_seconds: number;
  days: Day[];
  sessions: Session[];
}
const exec = promisify(execFile);
export function dateToday(): string {
  const date = new Date();
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
export function validDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T12:00:00`);
  return (
    !Number.isNaN(date.valueOf()) &&
    `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}` ===
      value
  );
}
export function moveDate(date: string, days: number): string {
  const value = new Date(`${date}T12:00:00`);
  value.setDate(value.getDate() + days);
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
}
export function duration(seconds: number): string {
  const value = Math.floor(Math.max(0, seconds));
  return `${Math.floor(value / 3600)}h ${String(Math.floor(value / 60) % 60).padStart(2, "0")}m ${String(value % 60).padStart(2, "0")}s`;
}
export function createBackend(
  options: { binary?: string; db?: string; exportDirectory?: string } = {},
) {
  const binary = options.binary ?? join(homedir(), ".local/bin/work-hours");
  async function run(args: string[]): Promise<string> {
    try {
      const { stdout } = await exec(
        binary,
        [...args, ...(options.db ? ["--db", options.db] : [])],
        { timeout: 12000, maxBuffer: 4 * 1024 * 1024 },
      );
      return stdout.trimEnd();
    } catch (error) {
      const e = error as { stderr?: string; code?: string };
      throw new Error(
        e.stderr?.trim().replace(/^work-hours: /, "") ||
          (e.code === "ENOENT"
            ? "Work Hours is not installed. Run ./install.sh."
            : "Work Hours command failed."),
      );
    }
  }
  return {
    snapshot: async (period: Period, date: string): Promise<Snapshot> =>
      JSON.parse(await run(["snapshot", "--period", period, "--date", date])),
    control: (action: "start" | "pause" | "toggle") => run([action]),
    saveNote: (date: string, text: string, expected: string) =>
      run([
        "note",
        "--date",
        date,
        `--text=${text}`,
        "--replace",
        `--expected-note=${expected}`,
      ]),
    saveSession: (start: string, end: string, id?: number) =>
      run([
        "session-save",
        "--start",
        start,
        "--end",
        end,
        ...(id !== undefined ? ["--id", String(id)] : []),
      ]),
    removeSession: (id: number) => run(["session-remove", "--id", String(id)]),
    restoreSession: (id: number) =>
      run(["session-restore", "--id", String(id)]),
    exportCsv: async (snapshot: Snapshot, kind: "daily" | "sessions") => {
      const documents = (
        process.env.XDG_DOCUMENTS_DIR || join(homedir(), "Documents")
      ).replace(/^\$HOME(?=\/|$)/, homedir());
      const directory =
        options.exportDirectory ?? join(documents, "Work Hours");
      await mkdir(directory, { recursive: true });
      const path = join(
        directory,
        `work-hours-${kind}-${snapshot.begin}-${snapshot.end}-${randomUUID()}.csv`,
      );
      await run([
        "export",
        "--from",
        snapshot.begin,
        "--to",
        snapshot.end,
        "--kind",
        kind,
        "--output",
        path,
      ]);
      return path;
    },
  };
}
export const backend = createBackend();
