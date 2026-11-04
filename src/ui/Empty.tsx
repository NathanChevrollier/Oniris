import type { CSSProperties, ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

/** État vide d'une carte : pastille teintée et une phrase courte, jamais un « Aucun » sec. */
export function Empty({ icon: Icon, tint, role, children }: { icon: LucideIcon; tint: string; role?: "status"; children: ReactNode }) {
  return (
    <div className="empty" role={role} style={{ "--tint": tint } as CSSProperties}>
      <span className="empty-ico" aria-hidden><Icon size={15} strokeWidth={1.9} /></span>
      <p>{children}</p>
    </div>
  );
}
