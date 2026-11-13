import type { InputHTMLAttributes, Ref } from "react";
import { Mic } from "lucide-react";
import { IrisOrb } from "../modules/iris/IrisReply";
import { useVoice, voiceIpc } from "../modules/voice/api";
import { hotkeyText, useSettings } from "../lib/settings";

/** Barre de commande (présentation) : la saisie et le clavier sont gérés par CommandPalette. */
export function CommandBar({ inputRef, onIris, onClaude, ...input }: InputHTMLAttributes<HTMLInputElement> & {
  inputRef: Ref<HTMLInputElement>; onIris?: () => void; onClaude?: () => void;
}) {
  const keys = hotkeyText(useSettings().hotkey).split(" + ");
  const listening = useVoice().phase === "listening";
  const typed = String(input.value ?? "").trim() !== "";
  const talk = listening ? "Arrêter l'écoute" : `Parler à IRIS (clic, ou maintiens ${keys.join("+")})`;
  // Le clic ne doit pas voler le focus de la saisie.
  const tap = { onMouseDown: (e: { preventDefault: () => void }) => e.preventDefault(), onClick: () => void voiceIpc.tap().catch(() => {}) };
  return (
    <div className="cmd">
      <button type="button" className="cmd-orb" aria-label={talk} title={talk} {...tap}><IrisOrb size={30} /></button>
      <input ref={inputRef} {...input} placeholder="Demande à IRIS, cherche, lance…" aria-label="Commande" autoComplete="off" spellCheck={false} />
      {typed && onIris && <button type="button" className="iris-chip" onMouseDown={(e) => e.preventDefault()} onClick={onIris}>IRIS</button>}
      {typed && onClaude && <button type="button" className="iris-chip" onMouseDown={(e) => e.preventDefault()} onClick={onClaude}>Claude</button>}
      <div className="k">
        {typed ? <><kbd>Ctrl</kbd><kbd>↵</kbd> demander à IRIS · </> : null}
        {keys.map((k) => <kbd key={k}>{k}</kbd>)} parler · <kbd>/</kbd> commandes
      </div>
      <button type="button" className="mic" data-on={listening || undefined} aria-label={talk} aria-pressed={listening} title={talk} {...tap}><Mic size={16} /></button>
    </div>
  );
}
