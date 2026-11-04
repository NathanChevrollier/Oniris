import type { CSSProperties, ReactNode } from "react";
import "./tokens.css";
import "./glass.css";
import "./motion.css";

export function Card({ title, aside, span, delay, children }: {
  title: string; aside?: ReactNode; span: 3 | 4 | 5 | 6 | 12; delay?: number; children?: ReactNode;
}) {
  const style: CSSProperties | undefined = delay === undefined ? undefined : { animationDelay: `${delay}s` };
  return (
    <section className={`card s${span}`} style={style}>
      <h3>{title}{aside != null && <span className="r">{aside}</span>}</h3>
      {children}
    </section>
  );
}
