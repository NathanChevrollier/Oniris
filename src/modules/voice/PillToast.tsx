import { useEffect, useState } from "react";
import { onToast } from "./api";
import "./voice.css";

export const TOAST_MS = 3000;

/** Un message à la fois, 3 s ; un nouveau remplace l'ancien. Texte brut. */
export function PillToast() {
  const [text, setText] = useState("");
  const [n, setN] = useState(0);
  useEffect(() => {
    const un = onToast((t) => { setText(t); setN((k) => k + 1); });
    return () => { void un.then((f) => f()); };
  }, []);
  useEffect(() => {
    if (!n) return;
    const t = setTimeout(() => setText(""), TOAST_MS);
    return () => clearTimeout(t);
  }, [n]);
  return <div className="pill-toast" role="status" aria-live="polite">{text}</div>;
}
