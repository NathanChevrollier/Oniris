import { useCallback, useEffect, useMemo, useState } from "react";
import { ipc, onConfirm, onPanelState, toolIpc } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { usePanelStore } from "./store";
import { focusHeaderButton, Header } from "./Header";
import { Timeline, type TimelineItem } from "./Timeline";
import { useTopic } from "../lib/data";
import { AgendaCard } from "../modules/agenda/AgendaCard";
import { TimerCard } from "../modules/timers/TimerCard";
import { NotesCard } from "../modules/notes/NotesCard";
import { PcCard } from "../modules/system/PcCard";
import { ClipboardCard } from "../modules/clipboard/ClipboardCard";
import { ProjectsCard } from "../modules/projects/ProjectsCard";
import { FavoritesCard } from "../modules/favorites/FavoritesCard";
import { IrisReply } from "../modules/iris/IrisReply";
import { MailCard } from "../modules/mail/MailCard";
import { Ambient, MusicCard } from "../modules/music/MusicCard";
import { useNow } from "../lib/time";
import { agendaIpc, dayBounds, dayKey, type AgendaEvent } from "../modules/agenda/api";
import { remindersIpc, type Reminder } from "../modules/reminders/api";
import { IrisSheet } from "../modules/iris/IrisSheet";
import { JournalSheet } from "../modules/iris/JournalSheet";
import { SettingsSheet } from "../modules/settings/SettingsSheet";
import { AccountsSheet } from "../modules/accounts/AccountsSheet";
import { Welcome } from "../modules/onboarding/Welcome";
import { CardGrid } from "./CardGrid";
import { CommandPalette } from "./CommandPalette";
import { ConfirmQueue } from "./ConfirmQueue";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./panel.css";

const NO_EVENTS: AgendaEvent[] = [];
const NO_REMINDERS: Reminder[] = [];
const NO_COLORS: string[] = [];

export function Panel() {
  const open = usePanelStore((s) => s.open);
  const [accountsOpen, setAccountsOpen] = useState(false);
  const openAccounts = useCallback(() => setAccountsOpen(true), []);
  const closeAccounts = useCallback(() => {
    setAccountsOpen(false);
    focusHeaderButton("accounts");
  }, []);
  const [irisOpen, setIrisOpen] = useState(false);
  const openIris = useCallback(() => setIrisOpen(true), []);
  const closeIris = useCallback(() => { setIrisOpen(false); focusHeaderButton("iris"); }, []);
  const [journalOpen, setJournalOpen] = useState(false);
  const openJournal = useCallback(() => setJournalOpen(true), []);
  const closeJournal = useCallback(() => { setJournalOpen(false); focusHeaderButton("journal"); }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => { setSettingsOpen(false); focusHeaderButton("settings"); }, []);
  const goFromSettings = useCallback((sheet: "iris" | "accounts") => { setSettingsOpen(false); (sheet === "iris" ? setIrisOpen : setAccountsOpen)(true); }, []);
  // Une confirmation arrive : la feuille ouverte se ferme, sinon la carte resterait inerte dessous.
  const pendingCount = usePanelStore((s) => s.pending.length);
  useEffect(() => {
    if (pendingCount === 0) return;
    setAccountsOpen(false); setIrisOpen(false); setJournalOpen(false); setSettingsOpen(false);
    // Le focus était dans la feuille (ou sur la carte, alors inerte) : on le rend à « Refuser », jamais à « Autoriser ».
    const t = setTimeout(() => document.querySelector<HTMLElement>("[data-confirm-refuse]")?.focus(), 0);
    return () => clearTimeout(t);
  }, [pendingCount]);
  const [welcome, setWelcome] = useState(false);
  const [colors, setColors] = useState<string[]>(NO_COLORS);
  const settings = useSettings();
  const now = useNow(60_000, open);
  // La clé du jour recharge la frise après minuit (ou à la réouverture le lendemain).
  const today = useTopic<AgendaEvent[]>("agenda", () => agendaIpc.between(...dayBounds(new Date())), NO_EVENTS, dayKey(now));
  // Les événements « journée entière » n'ont pas d'heure : ils restent dans la carte Agenda.
  const items = useMemo<TimelineItem[]>(() => today.data.filter((e) => !e.all_day).map((e) => ({
    start: new Date(e.start_ms), end: e.end_ms == null ? undefined : new Date(e.end_ms), label: e.title, kind: "event", color: e.color ?? undefined, readonly: e.readonly,
  })), [today.data]);

  const reminders = useTopic<Reminder[]>("reminders", remindersIpc.list, NO_REMINDERS, dayKey(now));
  const dots = useMemo<TimelineItem[]>(() => {
    const [from, to] = dayBounds(now);
    return reminders.data.filter((r) => !r.done && r.due_ms != null && r.due_ms >= from && r.due_ms < to)
      .map((r) => ({ start: new Date(r.due_ms!), label: r.text, kind: "reminder" as const }));
  }, [reminders.data, now]);

  useEffect(() => {
    const un = onPanelState((p) => usePanelStore.getState().setOpen(p.open));
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") ipc.panelClose(); };
    window.addEventListener("keydown", esc);
    return () => { un.then((f) => f()); window.removeEventListener("keydown", esc); };
  }, []);
  // Fermé : animations en pause (panel.css) et `rise` réarmée pour la prochaine ouverture.
  useEffect(() => { document.body.classList.toggle("closed", !open); }, [open]);

  return (
    <>
      <div className="scrim" onClick={() => ipc.panelClose()} />
      <section className="panel" aria-label="Oniris">
        <Ambient colors={colors} />
        <div className="inner" inert={welcome || accountsOpen || irisOpen || journalOpen || settingsOpen}>
          <Header name={settings.user_name} onAccounts={openAccounts} onIris={openIris} onJournal={openJournal} onSettings={openSettings} />
          <IrisReply />
          <Timeline startHour={settings.timeline_start_hour} endHour={settings.timeline_end_hour} items={[...items, ...dots]} />
          <ConfirmQueue subscribe={onConfirm} resolve={toolIpc.resolve} />
          <CardGrid slots={{ music: <MusicCard onAccounts={openAccounts} onColors={setColors} />, mails: <MailCard onAccounts={openAccounts} />, agenda: <AgendaCard />, projects: <ProjectsCard />, notes: <NotesCard />, clipboard: <ClipboardCard />, vps: <FavoritesCard />, pc: <PcCard />, timer: <TimerCard /> }} />
          <CommandPalette />
          <div className="handle" />
        </div>
        <Welcome covered={accountsOpen || irisOpen} onActive={setWelcome} onGo={goFromSettings} />
        <AccountsSheet open={accountsOpen} onClose={closeAccounts} onAutoOpen={openAccounts} />
        <IrisSheet open={irisOpen} onClose={closeIris} />
        <JournalSheet open={journalOpen} onClose={closeJournal} />
        <SettingsSheet open={settingsOpen} onClose={closeSettings} onGo={goFromSettings} />
      </section>
    </>
  );
}
