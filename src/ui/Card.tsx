import type { CSSProperties, ReactNode } from "react";
import "./tokens.css";
import "./glass.css";
import "./motion.css";

/** Carte : en-tête fixe ; dans la grille du panneau, le corps (`.card-body`) défile seul quand la rangée est trop basse. */
export function Card({ title, aside, span, delay, children }: {
  title: string; aside?: ReactNode; span: 3 | 4 | 5 | 6 | 12; delay?: number; children?: ReactNode;
}) {
  const style: CSSProperties | undefined = delay === undefined ? undefined : { animationDelay: `${delay}s` };
  return (
    <section className={`card s${span}`} style={style}>
      <h3>{title}{aside != null && <span className="r">{aside}</span>}</h3>
      <div className="card-body scroll-area">{children}</div>
    </section>
  );
}
