import { useEffect, useRef, useState, type FormEvent, type RefObject } from "react";
import { ChevronLeft, ChevronRight, MoreHorizontal } from "lucide-react";
import { isTauri } from "../../lib/ipc";
import { errorText } from "../accounts/api";
import { Card } from "../../ui/Card";
import { useTopic } from "../../lib/data";
import { useToolAction } from "../../lib/toolAction";
import { useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { agendaIpc, calendarsIpc, dayBounds, dayKey, reportText, type AgendaEvent } from "./api";
import { CalendarsManager } from "./Calendars";
import { ReminderList } from "../reminders/ReminderList";
import { MonthGrid } from "./MonthGrid";
import { DayList } from "./DayList";
import "./agenda.css";

const MONTHS = ["Janvier", "Février", "Mars", "Avril", "Mai", "Juin", "Juillet", "Août", "Septembre", "Octobre", "Novembre", "Décembre"];
const UNAVAILABLE = "Agenda indisponible, réessaie.";

type View = { days: number[]; events: AgendaEvent[] };
type Pos = { x: number; y: number };

/** Fichiers .ics déposés sur la carte (glisser-déposer de Tauri : chemins et position en pixels physiques). */
function useIcsDrop(target: RefObject<HTMLElement | null>, onFiles: (paths: string[]) => void) {
  const [over, setOver] = useState(false);
  const cb = useRef(onFiles);
  cb.current = onFiles;
  useEffect(() => {
    if (!isTauri) return;
    let un: (() => void) | undefined;
    let gone = false;
    const inside = (p: Pos) => {
      const r = target.current?.getBoundingClientRect();
      const [x, y] = [p.x / devicePixelRatio, p.y / devicePixelRatio];
      return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    };
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent(({ payload: p }) => {
      if (p.type === "enter" || p.type === "over") setOver(inside(p.position));
      else if (p.type === "drop") {
        setOver(false);
        const files = p.paths.filter((f) => /\.ics$/i.test(f));
        if (files.length && inside(p.position)) cb.current(files);
      } else setOver(false);
    })).then((f) => { if (gone) f(); else un = f; }, () => {});
    return () => { gone = true; un?.(); };
  }, [target]);
  return over;
}

/** Fenêtre « Calendriers affichés » de la carte : se ferme au clic dehors et sur Échap. */
function CalendarsPop({ onClose }: { onClose: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const out = (e: Event) => { if (!root.current?.contains(e.target as Node) && !(e.target as Element).closest?.(".ag-more")) onClose(); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    document.addEventListener("pointerdown", out);
    document.addEventListener("keydown", esc, true);
    return () => { document.removeEventListener("pointerdown", out); document.removeEventListener("keydown", esc, true); };
  }, [onClose]);
  return (
    <div ref={root} className="ag-pop scroll-area" role="dialog" aria-label="Calendriers affichés">
      <CalendarsManager />
    </div>
  );
}

export function AgendaCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  // Sans choix de l'utilisateur, le jour affiché suit « aujourd'hui » (y compris après minuit).
  const [picked, setPicked] = useState<Date | null>(null);
  const sel = picked ?? now;
  const [text, setText] = useState("");
  const { run, error, clear } = useToolAction(UNAVAILABLE);
  const year = sel.getFullYear();
  const month = sel.getMonth() + 1;

  const { data: view } = useTopic<View | null>("agenda", async () => {
    const [from, to] = dayBounds(sel);
    const [days, events] = await Promise.all([agendaIpc.monthDays(year, month), agendaIpc.between(from, to)]);
    return { days, events };
  }, null, dayKey(sel));
  const data = view ?? { days: [], events: [] };
  const [cals, setCals] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  // Abonnements en ligne : relevés au plus toutes les 6 h, seulement panneau ouvert (Rust décide).
  useEffect(() => { if (open) void calendarsIpc.refresh().catch(() => {}); }, [open]);
  const box = useRef<HTMLDivElement>(null);
  const over = useIcsDrop(box, (paths) => void (async () => {
    const lines: string[] = [];
    for (const p of paths.slice(0, 5)) {
      try { lines.push(reportText(await calendarsIpc.importPath(p))); }
      catch (e) { setNote({ ok: false, text: errorText(e, "Import impossible, réessaie.") }); return; }
    }
    setNote({ ok: true, text: `Importé — ${lines.join(" · ")}` });
  })());

  const isNow = now.getFullYear() === year && now.getMonth() + 1 === month;
  const go = (delta: number) => {
    const d = new Date(year, month - 1 + delta, 1);
    setPicked(d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() ? null : d);
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (t && (await run(agendaIpc.create(t)))) setText("");
  };
  const remove = (id: number) => run(agendaIpc.remove(id));

  return (
    <Card title={MONTHS[month - 1] + (year === now.getFullYear() ? "" : ` ${year}`)} span={3} delay={0.15}
      aside={<span className="nav">
        <button type="button" aria-label="Mois précédent" onClick={() => go(-1)}><ChevronLeft size={15} /></button>
        <button type="button" aria-label="Mois suivant" onClick={() => go(1)}><ChevronRight size={15} /></button>
        <button type="button" className="ag-more" aria-label="Calendriers affichés" title="Calendriers affichés" aria-expanded={cals}
          onClick={() => setCals(!cals)}><MoreHorizontal size={15} /></button>
      </span>}>
      {/* Le mois reste en place ; la journée et les rappels défilent dessous. */}
      <div className={over ? "ag drop" : "ag"} ref={box}>
        {cals && <CalendarsPop onClose={() => setCals(false)} />}
        {over && <p className="ag-drop" aria-hidden>Dépose le fichier .ics pour l'importer</p>}
        <MonthGrid year={year} month={month} today={isNow ? now.getDate() : null} selected={sel.getDate()}
          withEvents={data.days} onSelect={(day) => setPicked(new Date(year, month - 1, day))} />
        <div className="ag-day scroll-area">
          <DayList events={data.events} onDelete={remove} loading={view === null} />
          <div aria-live="polite">{note && <p className={note.ok ? "ag-note" : "ag-note err"}>{note.text}</p>}</div>
          <form className="add" onSubmit={submit}>
            <input value={text} onChange={(e) => { setText(e.target.value); clear(); }} placeholder="Ajouter…"
              aria-label="Ajouter un événement" autoComplete="off" spellCheck={false} />
            {error && <p className="err" role="alert">{error}</p>}
          </form>
          <ReminderList />
        </div>
      </div>
    </Card>
  );
}
