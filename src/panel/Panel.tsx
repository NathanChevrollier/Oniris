import { useEffect } from "react";
import { Sparkles } from "lucide-react";
import { ipc, onConfirm, onPanelState, toolIpc } from "../lib/ipc";
import { useSettings } from "../lib/settings";
import { usePanelStore } from "./store";
import { Header } from "./Header";
import { Timeline } from "./Timeline";
import { CardGrid } from "./CardGrid";
import { CommandBar } from "./CommandBar";
import { ConfirmQueue } from "./ConfirmQueue";
import "../ui/tokens.css";
import "../ui/glass.css";
import "../ui/motion.css";
import "./panel.css";

const logCommand = (text: string) => console.info("[oniris] commande :", text); // branchée sur IRIS au plan 5

export function Panel({ onCommand = logCommand }: { onCommand?: (text: string) => void }) {
  const open = usePanelStore((s) => s.open);
  const settings = useSettings();

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
          <Timeline startHour={settings.timeline_start_hour} endHour={settings.timeline_end_hour} items={[]} />
          <ConfirmQueue subscribe={onConfirm} resolve={toolIpc.resolve} />
          <CardGrid />
          <CommandBar onSubmit={onCommand} />
          <div className="handle" />
        </div>
      </section>
    </>
  );
}
