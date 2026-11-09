import {
  Action,
  ActionPanel,
  Alert,
  Detail,
  Form,
  Icon,
  List,
  Toast,
  confirmAlert,
  showToast,
  useNavigation,
} from "@vicinae/api";
import { useEffect, useRef, useState } from "react";
import {
  backend,
  dateToday,
  duration,
  moveDate,
  validDate,
  type Day,
  type Period,
  type Session,
  type Snapshot,
} from "./backend";

type Refresh = () => Promise<void>;
async function failure(error: unknown) {
  await showToast({
    style: Toast.Style.Failure,
    title: "Work Hours",
    message: error instanceof Error ? error.message : String(error),
  });
}
function useSnapshot(period: Period, date: string) {
  const [data, setData] = useState<Snapshot>();
  const [error, setError] = useState<string>();
  const version = useRef(0);
  useEffect(() => {
    const current = ++version.current;
    let busy = false;
    setData(undefined);
    const load = async () => {
      if (busy) return;
      busy = true;
      try {
        const value = await backend.snapshot(period, date);
        if (version.current === current) {
          setData(value);
          setError(undefined);
        }
      } catch (e) {
        if (version.current === current)
          setError(e instanceof Error ? e.message : String(e));
      } finally {
        busy = false;
      }
    };
    void load();
    const timer = setInterval(() => void load(), 1000);
    return () => {
      ++version.current;
      clearInterval(timer);
    };
  }, [period, date]);
  const refresh: Refresh = async () => {
    const current = version.current;
    const value = await backend.snapshot(period, date);
    if (version.current === current) {
      setData(value);
      setError(undefined);
    }
  };
  return { data, error, refresh };
}
function NoteForm({ day, refresh }: { day: Day; refresh: Refresh }) {
  const { pop } = useNavigation();
  const [saving, setSaving] = useState(false);
  return (
    <Form
      navigationTitle={`Notes for ${day.date}`}
      isLoading={saving}
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Save Note"
            onSubmit={async (values: Form.Values) => {
              if (saving) return;
              setSaving(true);
              try {
                await backend.saveNote(
                  day.date,
                  String(values.note ?? ""),
                  day.note,
                );
                await refresh();
                pop();
                await showToast({ title: "Note saved" });
              } catch (e) {
                await failure(e);
              } finally {
                setSaving(false);
              }
            }}
          />
        </ActionPanel>
      }
    >
      <Form.Description
        text={`Edit the complete note for ${day.date}. Saving an empty note clears it.`}
      />
      <Form.TextArea
        id="note"
        title="Daily Notes"
        defaultValue={day.note}
        placeholder="What did you work on?"
      />
    </Form>
  );
}
function SessionForm({
  day,
  session,
  refresh,
}: {
  day: string;
  session?: Session;
  refresh: Refresh;
}) {
  const { pop } = useNavigation();
  const [saving, setSaving] = useState(false);
  return (
    <Form
      navigationTitle={
        session ? `Edit session #${session.id}` : `Add session for ${day}`
      }
      isLoading={saving}
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Save Session"
            onSubmit={async (values: Form.Values) => {
              if (saving) return;
              setSaving(true);
              try {
                await backend.saveSession(
                  String(values.start ?? ""),
                  String(values.end ?? ""),
                  session?.id,
                );
                await refresh();
                pop();
                await showToast({ title: "Session saved" });
              } catch (e) {
                await failure(e);
              } finally {
                setSaving(false);
              }
            }}
          />
        </ActionPanel>
      }
    >
      <Form.Description text="Use YYYY-MM-DD HH:MM or ISO times with UTC offsets. Overlapping sessions and future end times are rejected. Pause a running session before editing it." />
      <Form.TextField
        id="start"
        title="Start"
        defaultValue={session?.start ?? `${day} 09:00`}
      />
      <Form.TextField
        id="end"
        title="End"
        defaultValue={session?.end ?? `${day} 10:00`}
      />
    </Form>
  );
}
function DateForm({
  date,
  onSelect,
}: {
  date: string;
  onSelect: (date: string) => void;
}) {
  const { pop } = useNavigation();
  const [error, setError] = useState<string>();
  return (
    <Form
      navigationTitle="Go to Date"
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Show Date"
            onSubmit={(values: Form.Values) => {
              if (!validDate(String(values.date ?? ""))) {
                setError("Enter a valid date as YYYY-MM-DD");
                return;
              }
              onSelect(String(values.date));
              pop();
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField
        id="date"
        title="Date"
        defaultValue={date}
        error={error}
      />
    </Form>
  );
}
function ExportResult({ path }: { path: string }) {
  return (
    <Detail
      navigationTitle="CSV Exported"
      markdown={`# CSV exported\n\nSaved to:\n\n\`${path}\`\n\nIncludes the selected period's hours and daily notes.`}
      actions={
        <ActionPanel>
          <Action.CopyToClipboard title="Copy File Path" content={path} />
          <Action.ShowInFinder path={path} />
        </ActionPanel>
      }
    />
  );
}
function CommonActions({
  snapshot,
  day,
  refresh,
}: {
  snapshot: Snapshot;
  day: Day;
  refresh: Refresh;
}) {
  const { push } = useNavigation();
  const [pending, setPending] = useState(false);
  async function toggle() {
    if (pending) return;
    setPending(true);
    try {
      await backend.control(snapshot.running ? "pause" : "start");
      await refresh();
    } catch (e) {
      await failure(e);
    } finally {
      setPending(false);
    }
  }
  async function exportCsv(kind: "daily" | "sessions") {
    try {
      const path = await backend.exportCsv(snapshot, kind);
      push(<ExportResult path={path} />);
    } catch (e) {
      await failure(e);
    }
  }
  return (
    <>
      <ActionPanel.Section title="Work">
        <Action
          title={snapshot.running ? "Pause Working" : "Start Working"}
          icon={snapshot.running ? Icon.Pause : Icon.Play}
          shortcut={{ modifiers: ["ctrl"], key: "t" }}
          onAction={toggle}
        />
        <Action.Push
          title="Edit Daily Note"
          icon={Icon.Pencil}
          shortcut={{ modifiers: ["ctrl"], key: "n" }}
          target={<NoteForm day={day} refresh={refresh} />}
        />
        <Action.Push
          title="Add Session"
          icon={Icon.Plus}
          target={<SessionForm day={day.date} refresh={refresh} />}
        />
      </ActionPanel.Section>
      <ActionPanel.Section title={`Export ${snapshot.begin} — ${snapshot.end}`}>
        <Action
          title="Export Days CSV"
          icon={Icon.BlankDocument}
          onAction={() => exportCsv("daily")}
        />
        <Action
          title="Export Sessions CSV"
          icon={Icon.BlankDocument}
          onAction={() => exportCsv("sessions")}
        />
      </ActionPanel.Section>
      <Action
        title="Refresh"
        icon={Icon.ArrowClockwise}
        shortcut={{ modifiers: ["ctrl"], key: "r" }}
        onAction={async () => {
          try {
            await refresh();
          } catch (e) {
            await failure(e);
          }
        }}
      />
    </>
  );
}
function DayView({ date }: { date: string }) {
  const { data, error, refresh } = useSnapshot("day", date);
  const [removed, setRemoved] = useState<number>();
  if (!data)
    return (
      <List isLoading={!error}>
        <List.EmptyView
          title={error ? "Couldn't load Work Hours" : "Loading"}
          description={error}
        />
      </List>
    );
  const day = data.days[0];
  const common = <CommonActions snapshot={data} day={day} refresh={refresh} />;
  return (
    <List
      navigationTitle={`${date} · ${duration(day.seconds)}`}
      isShowingDetail
      searchBarPlaceholder="Search sessions…"
    >
      <List.Item
        id="notes"
        title="Daily Notes"
        subtitle={duration(day.seconds)}
        icon={Icon.BlankDocument}
        detail={
          <List.Item.Detail
            markdown={
              day.note ||
              "No notes yet. Choose **Edit Daily Note** to add them."
            }
          />
        }
        actions={
          <ActionPanel>
            <Action.Push
              title="Edit Daily Note"
              icon={Icon.Pencil}
              target={<NoteForm day={day} refresh={refresh} />}
            />
            {common}
          </ActionPanel>
        }
      />
      {data.sessions.map((session) => (
        <List.Item
          key={session.id}
          id={String(session.id)}
          title={`Session #${session.id}${session.end ? "" : " · Running"}`}
          subtitle={duration(session.seconds)}
          icon={session.end ? Icon.Clock : Icon.Play}
          detail={
            <List.Item.Detail
              markdown={`**Start:** ${session.start}\n\n**End:** ${session.end ?? "Running — pause to finish"}\n\n**Duration:** ${duration(session.seconds)}`}
            />
          }
          actions={
            <ActionPanel>
              {session.end ? (
                <Action.Push
                  title="Edit Session"
                  icon={Icon.Pencil}
                  target={
                    <SessionForm
                      day={date}
                      session={session}
                      refresh={refresh}
                    />
                  }
                />
              ) : (
                <Action
                  title="Pause Working"
                  icon={Icon.Pause}
                  onAction={async () => {
                    try {
                      await backend.control("pause");
                      await refresh();
                    } catch (e) {
                      await failure(e);
                    }
                  }}
                />
              )}
              {common}
              {session.end && (
                <Action
                  title="Remove Session"
                  icon={Icon.Trash}
                  style={Action.Style.Destructive}
                  onAction={async () => {
                    if (
                      await confirmAlert({
                        title: `Remove session #${session.id}?`,
                        message:
                          "You can undo this removal from this day's action menu.",
                        primaryAction: {
                          title: "Remove",
                          style: Alert.ActionStyle.Destructive,
                        },
                      })
                    ) {
                      try {
                        await backend.removeSession(session.id);
                        setRemoved(session.id);
                        await refresh();
                        await showToast({ title: "Session removed" });
                      } catch (e) {
                        await failure(e);
                      }
                    }
                  }}
                />
              )}
            </ActionPanel>
          }
        />
      ))}
      {removed !== undefined && (
        <List.Item
          title="Undo Last Removal"
          icon={Icon.ArrowCounterClockwise}
          actions={
            <ActionPanel>
              <Action
                title="Restore Session"
                onAction={async () => {
                  try {
                    await backend.restoreSession(removed);
                    setRemoved(undefined);
                    await refresh();
                  } catch (e) {
                    await failure(e);
                  }
                }}
              />
            </ActionPanel>
          }
        />
      )}
    </List>
  );
}
export default function WorkHours() {
  const [period, setPeriod] = useState<Period>("week");
  const [date, setDate] = useState(dateToday);
  const { data, error, refresh } = useSnapshot(period, date);
  const navigate = (direction: number) => {
    if (!data) return;
    setDate(direction > 0 ? moveDate(data.end, 1) : moveDate(data.begin, -1));
  };
  const selectedDay =
    data?.days.find((day) => day.date === date) ?? data?.days[0];
  const navigation = (
    <ActionPanel.Section title="History">
      <Action
        title="Previous Period"
        icon={Icon.ArrowLeft}
        onAction={() => navigate(-1)}
      />
      <Action
        title="Next Period"
        icon={Icon.ArrowRight}
        onAction={() => navigate(1)}
      />
      <Action
        title="Go to Today"
        icon={Icon.Calendar}
        onAction={() => setDate(dateToday())}
      />
      <Action.Push
        title="Go to Date"
        icon={Icon.Calendar}
        target={<DateForm date={date} onSelect={setDate} />}
      />
    </ActionPanel.Section>
  );
  return (
    <List
      isLoading={!data && !error}
      isShowingDetail
      searchBarPlaceholder="Search work days…"
      searchBarAccessory={
        <List.Dropdown
          tooltip="Summary period"
          value={period}
          onChange={(value) => setPeriod(value as Period)}
        >
          <List.Dropdown.Item title="Day" value="day" />
          <List.Dropdown.Item title="Week" value="week" />
          <List.Dropdown.Item title="Month" value="month" />
        </List.Dropdown>
      }
    >
      {error && (
        <List.Item
          title="Couldn't load Work Hours"
          subtitle={error}
          icon={Icon.Warning}
        />
      )}
      {data && selectedDay && (
        <>
          <List.Item
            id="timer"
            title={
              data.running ? "Working — Pause Timer" : "Paused — Start Timer"
            }
            subtitle={duration(data.total_seconds)}
            icon={data.running ? Icon.Play : Icon.Pause}
            detail={
              <List.Item.Detail
                markdown={`# ${data.running ? "Working" : "Paused"}\n\n**${data.begin} — ${data.end}**\n\nTotal: **${duration(data.total_seconds)}**\n\nEnter to ${data.running ? "pause" : "start"}. Select a day to edit notes and sessions.\n\nPauses when idle, asleep, or shut down. Start it again when you return.`}
              />
            }
            actions={
              <ActionPanel>
                <CommonActions
                  snapshot={data}
                  day={selectedDay}
                  refresh={refresh}
                />
                {navigation}
              </ActionPanel>
            }
          />
          <List.Section
            title={`${data.begin} — ${data.end}`}
            subtitle={`Total ${duration(data.total_seconds)}`}
          >
            {data.days.map((day) => (
              <List.Item
                key={day.date}
                id={day.date}
                title={day.date}
                subtitle={duration(day.seconds)}
                icon={day.note ? Icon.BlankDocument : Icon.Calendar}
                detail={
                  <List.Item.Detail
                    markdown={`# ${day.date}\n\n**${duration(day.seconds)}**\n\n${day.note || "No daily notes yet."}`}
                  />
                }
                actions={
                  <ActionPanel>
                    <Action.Push
                      title="Open Day — Notes and Sessions"
                      icon={Icon.Calendar}
                      target={<DayView date={day.date} />}
                    />
                    <CommonActions
                      snapshot={data}
                      day={day}
                      refresh={refresh}
                    />
                    {navigation}
                  </ActionPanel>
                }
              />
            ))}
          </List.Section>
        </>
      )}
    </List>
  );
}
