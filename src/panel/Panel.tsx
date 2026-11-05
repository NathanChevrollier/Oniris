import { useCallback, useEffect, useMemo, useState } from "react";
import { Sparkles } from "lucide-react";
import { ipc, onConfirm, onPanelState, toolIpc } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { usePanelStore } from "./store";
import { Header } from "./Header";
import { Timeline, type TimelineItem } from "./Timeline";
import { useTopic } from "../lib/data";
import { AgendaCard } from "../modules/agenda/AgendaCard";
import { TimerCard } from "../modules/timers/TimerCard";
import { NotesCard } from "../modules/notes/NotesCard";
import { PcCard } from "../modules/system/PcCard";
import { ClipboardCard } from "../modules/clipboard/ClipboardCard";
import { ProjectsCard } from "../modules/projects/ProjectsCard";
import { FavoritesCard } from "../modules/favorites/FavoritesCard";
import { MailCard } from "../modules/mail/MailCard";
import { Ambient, MusicCard } from "../modules/music/MusicCard";
import { useNow } from "../lib/time";
import { agendaIpc, dayBounds, dayKey, type AgendaEvent } from "../modules/agenda/api";
import { remindersIpc, type Reminder } from "../modules/reminders/api";
import { AccountsSheet } from "../modules/accounts/AccountsSheet";
import { CardGrid } from "./CardGrid";
import { CommandPalette } from "./CommandPalette";
import { ConfirmQueue } from "./ConfirmQueue";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./panel.css";

const logCommand = (text: string) => console.info("[oniris] commande :", text); // branchée sur IRIS au plan 5

const NO_EVENTS: AgendaEvent[] = [];
const NO_REMINDERS: Reminder[] = [];
const NO_COLORS: string[] = [];

export function Panel({ onCommand = logCommand }: { onCommand?: (text: string) => void }) {
  const open = usePanelStore((s) => s.open);
  const [accountsOpen, setAccountsOpen] = useState(false);
  const openAccounts = useCallback(() => setAccountsOpen(true), []);
  const closeAccounts = useCallback(() => {
    setAccountsOpen(false);
    // Le contenu derrière la feuille redevient actif au rendu suivant : on lui rend le focus après.
    setTimeout(() => document.querySelector<HTMLElement>(".hdr-btn")?.focus(), 0);
  }, []);
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
        <div className="inner" inert={accountsOpen}>
          <Header name={settings.user_name} onAccounts={openAccounts} />
          <div className="brief">
            <div className="spark"><Sparkles size={15} /></div>
            <p>IRIS arrive bientôt.</p>
          </div>
          <Timeline startHour={settings.timeline_start_hour} endHour={settings.timeline_end_hour} items={[...items, ...dots]} />
          <ConfirmQueue subscribe={onConfirm} resolve={toolIpc.resolve} />
          <CardGrid slots={{ music: <MusicCard onAccounts={openAccounts} onColors={setColors} />, mails: <MailCard onAccounts={openAccounts} />, agenda: <AgendaCard />, projects: <ProjectsCard />, notes: <NotesCard />, clipboard: <ClipboardCard />, vps: <FavoritesCard />, pc: <PcCard />, timer: <TimerCard /> }} />
          <CommandPalette onCommand={onCommand} />
          <div className="handle" />
        </div>
        <AccountsSheet open={accountsOpen} onClose={closeAccounts} onAutoOpen={openAccounts} />
      </section>
    </>
  );
}
