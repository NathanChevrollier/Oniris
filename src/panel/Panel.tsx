import { useEffect, useMemo } from "react";
import { Sparkles } from "lucide-react";
import { ipc, onConfirm, onPanelState, toolIpc } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { usePanelStore } from "./store";
import { Header } from "./Header";
import { Timeline, type TimelineItem } from "./Timeline";
import { useTopic } from "../lib/data";
import { AgendaCard } from "../modules/agenda/AgendaCard";
import { useNow } from "../lib/time";
import { agendaIpc, dayBounds, dayKey, type AgendaEvent } from "../modules/agenda/api";
import { CardGrid } from "./CardGrid";
import { CommandBar } from "./CommandBar";
import { ConfirmQueue } from "./ConfirmQueue";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./panel.css";

const logCommand = (text: string) => console.info("[oniris] commande :", text); // branchée sur IRIS au plan 5

const NO_EVENTS: AgendaEvent[] = [];

export function Panel({ onCommand = logCommand }: { onCommand?: (text: string) => void }) {
  const open = usePanelStore((s) => s.open);
  const settings = useSettings();
  const now = useNow(60_000, open);
  // La clé du jour recharge la frise après minuit (ou à la réouverture le lendemain).
  const today = useTopic<AgendaEvent[]>("agenda", () => agendaIpc.between(...dayBounds(new Date())), NO_EVENTS, dayKey(now));
  // Les événements « journée entière » n'ont pas d'heure : ils restent dans la carte Agenda.
  const items = useMemo<TimelineItem[]>(() => today.data.filter((e) => !e.all_day).map((e) => ({
    start: new Date(e.start_ms), end: e.end_ms == null ? undefined : new Date(e.end_ms), label: e.title, kind: "event", color: e.color ?? undefined,
  })), [today.data]);

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
        <div className="ambient"><i /><i /><i /></div>
        <div className="inner">
          <Header name={settings.user_name} />
          <div className="brief">
            <div className="spark"><Sparkles size={15} /></div>
            <p>IRIS arrive bientôt.</p>
          </div>
          <Timeline startHour={settings.timeline_start_hour} endHour={settings.timeline_end_hour} items={items} />
          <ConfirmQueue subscribe={onConfirm} resolve={toolIpc.resolve} />
          <CardGrid slots={{ agenda: <AgendaCard /> }} />
          <CommandBar onSubmit={onCommand} />
          <div className="handle" />
        </div>
      </section>
    </>
  );
}
