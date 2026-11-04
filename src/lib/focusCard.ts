import { useEffect, type RefObject } from "react";

const EVENT = "oniris:focus-card";
type Detail = { card: string; query: string };

/** Fait venir une carte et place le focus dans sa recherche, pré-remplie avec `query` (palette de commande). */
export const focusCard = (card: "notes" | "clipboard", query = "") =>
  window.dispatchEvent(new CustomEvent<Detail>(EVENT, { detail: { card, query } }));

/** Côté carte : `input` est son champ de recherche, `setQuery` son état. */
export function useFocusCard(card: string, input: RefObject<HTMLInputElement | null>, setQuery: (q: string) => void) {
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<Detail>).detail;
      if (d.card !== card) return;
      setQuery(d.query);
      input.current?.scrollIntoView?.({ block: "nearest" });
      input.current?.focus({ preventScroll: true });
    };
    window.addEventListener(EVENT, on);
    return () => window.removeEventListener(EVENT, on);
  }, [card, input, setQuery]);
}
