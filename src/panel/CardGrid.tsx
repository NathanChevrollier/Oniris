import type { CSSProperties, ReactNode } from "react";
import { CalendarDays, ClipboardList, Cpu, FolderGit2, Mail, Music, Server, StickyNote, Timer, type LucideIcon } from "lucide-react";
import { Card } from "../ui/Card";

export type SlotId = "music" | "mails" | "agenda" | "projects" | "pc" | "vps" | "notes" | "timer" | "clipboard";

// Disposition de la maquette (3 rangées de 12 colonnes) ; chaque plan remplace son emplacement par sa carte.
const SLOTS: Record<SlotId, { title: string; span: 3 | 4 | 5 | 6; icon: LucideIcon; tint: string }> = {
  music: { title: "En lecture", span: 4, icon: Music, tint: "#ff7448" },
  mails: { title: "Mails", span: 5, icon: Mail, tint: "#0a84ff" },
  agenda: { title: "Agenda", span: 3, icon: CalendarDays, tint: "#ffd60a" },
  projects: { title: "Projets", span: 6, icon: FolderGit2, tint: "#30d158" },
  pc: { title: "Ce PC", span: 3, icon: Cpu, tint: "#64d2ff" },
  vps: { title: "VPS · sites", span: 3, icon: Server, tint: "#bf5af2" },
  notes: { title: "Notes", span: 4, icon: StickyNote, tint: "#ffd60a" },
  clipboard: { title: "Presse-papiers", span: 5, icon: ClipboardList, tint: "#64d2ff" },
  timer: { title: "Minuteur", span: 3, icon: Timer, tint: "#ff7448" },
};
const ORDER: SlotId[] = ["music", "mails", "agenda", "projects", "pc", "vps", "notes", "clipboard", "timer"];

export function SoonCard({ id, delay }: { id: SlotId; delay?: number }) {
  const { title, span, icon: Icon, tint } = SLOTS[id];
  return (
    <Card title={title} span={span} delay={delay}>
      <div className="soon" style={{ "--tint": tint } as CSSProperties}>
        <span className="soon-ico"><Icon size={22} strokeWidth={1.75} /></span>
        <span>Bientôt</span>
      </div>
    </Card>
  );
}

/** Grille de 12 colonnes ; un emplacement sans carte affiche « Bientôt ». */
export function CardGrid({ slots = {} }: { slots?: Partial<Record<SlotId, ReactNode>> }) {
  return (
    <div className="grid">
      {ORDER.map((id, i) => slots[id] ?? <SoonCard key={id} id={id} delay={0.05 * (i + 1)} />)}
    </div>
  );
}
