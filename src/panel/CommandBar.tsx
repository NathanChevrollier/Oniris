import type { InputHTMLAttributes, Ref } from "react";
import { Mic } from "lucide-react";
import { IrisOrb } from "../modules/iris/IrisReply";

/** Barre de commande (présentation) : la saisie et le clavier sont gérés par CommandPalette. */
export function CommandBar({ inputRef, onClaude, ...input }: InputHTMLAttributes<HTMLInputElement> & { inputRef: Ref<HTMLInputElement>; onClaude?: () => void }) {
  return (
    <div className="cmd">
      <IrisOrb size={30} />
      <input ref={inputRef} {...input} placeholder="Demande, cherche, lance…" aria-label="Commande" autoComplete="off" spellCheck={false} />
      {onClaude && String(input.value ?? "").trim() !== "" && (
        <button type="button" className="iris-chip" onMouseDown={(e) => e.preventDefault()} onClick={onClaude}>Claude</button>
      )}
      <div className="k">{onClaude && String(input.value ?? "").trim() !== "" && <><kbd>Ctrl</kbd><kbd>↵</kbd> demander à IRIS · </>}<kbd>Alt</kbd><kbd>Espace</kbd> parler · <kbd>/</kbd> commandes</div>
      <span className="mic" role="img" aria-label="Maintiens Alt+Espace pour parler" title="Maintiens Alt+Espace pour parler"><Mic size={16} /></span>
    </div>
  );
}
