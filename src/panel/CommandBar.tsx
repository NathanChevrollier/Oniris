import type { InputHTMLAttributes, Ref } from "react";
import { Mic } from "lucide-react";
import { Orb } from "../ui/Orb";

/** Barre de commande (présentation) : la saisie et le clavier sont gérés par CommandPalette. */
export function CommandBar({ inputRef, ...input }: InputHTMLAttributes<HTMLInputElement> & { inputRef: Ref<HTMLInputElement> }) {
  return (
    <div className="cmd">
      <Orb state="idle" size={30} />
      <input ref={inputRef} {...input} placeholder="Demande, cherche, lance…" aria-label="Commande" autoComplete="off" spellCheck={false} />
      <div className="k"><kbd>Alt</kbd><kbd>Espace</kbd> parler · <kbd>/</kbd> commandes</div>
      <button type="button" className="mic" aria-label="Parler (bientôt)" disabled><Mic size={16} /></button>
    </div>
  );
}
