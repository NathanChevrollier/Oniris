import { useEffect, useRef, useState } from "react";
import { Mic } from "lucide-react";
import { Orb } from "../ui/Orb";
import { usePanelStore } from "./store";

export function CommandBar({ onSubmit }: { onSubmit: (text: string) => void }) {
  const open = usePanelStore((s) => s.open);
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  useEffect(() => { if (open) input.current?.focus(); }, [open]);
  return (
    <form className="cmd" onSubmit={(e) => {
      e.preventDefault();
      const t = text.trim();
      if (t) { onSubmit(t); setText(""); }
    }}>
      <Orb state="idle" size={30} />
      <input ref={input} value={text} onChange={(e) => setText(e.target.value)}
        placeholder="Demande, cherche, lance…" aria-label="Commande" autoComplete="off" spellCheck={false} />
      <div className="k"><kbd>Alt</kbd><kbd>Espace</kbd> parler · <kbd>/</kbd> commandes</div>
      <button type="button" className="mic" aria-label="Parler (bientôt)" disabled><Mic size={16} /></button>
    </form>
  );
}
