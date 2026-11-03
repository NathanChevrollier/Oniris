import type { CSSProperties, ReactNode } from "react";
import { CalendarDays, Cpu, FolderGit2, Mail, Music, Server, type LucideIcon } from "lucide-react";
import { Card } from "../ui/Card";

// Disposition de la maquette ; les plans 2 à 4 remplacent ces emplacements par leurs cartes.
const SLOTS: { title: string; span: 3 | 4 | 5 | 6; icon: LucideIcon; tint: string }[] = [
  { title: "En lecture", span: 4, icon: Music, tint: "#ff7448" },
  { title: "Mails", span: 5, icon: Mail, tint: "#0a84ff" },
  { title: "Agenda", span: 3, icon: CalendarDays, tint: "#ffd60a" },
  { title: "Projets", span: 6, icon: FolderGit2, tint: "#30d158" },
  { title: "Ce PC", span: 3, icon: Cpu, tint: "#64d2ff" },
  { title: "VPS · sites", span: 3, icon: Server, tint: "#bf5af2" },
];

/** Grille de 12 colonnes ; sans enfants, affiche les emplacements « Bientôt ». */
export function CardGrid({ children }: { children?: ReactNode }) {
  return (
    <div className="grid">
      {children ?? SLOTS.map(({ title, span, icon: Icon, tint }, i) => (
        <Card key={title} title={title} span={span} delay={0.05 * (i + 1)}>
          <div className="soon" style={{ "--tint": tint } as CSSProperties}>
            <span className="soon-ico"><Icon size={22} strokeWidth={1.75} /></span>
            <span>Bientôt</span>
          </div>
        </Card>
      ))}
    </div>
  );
}
