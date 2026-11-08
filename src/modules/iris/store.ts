import { create } from "zustand";
import { irisIpc, type Brain, type IrisEvent, type IrisErrorKind, type IrisStatus, type Segment } from "./api";

type IrisError = { kind: IrisErrorKind; message: string; offer_local: boolean };

export type IrisStore = {
  /** Tour courant (0 : aucun) ; les événements d'un tour ≤ `floor` (tour abandonné ou périmé) sont ignorés. */
  turn: number;
  floor: number;
  state: "idle" | "thinking";
  brain: Brain | null;
  /** Dernier message envoyé ; vide = aucun échange affiché (le briefing reprend la place). */
  user: string;
  segments: Segment[];
  pending: number[];
  error: IrisError | null;
  toolsUsed: string[];
  status: IrisStatus | null;
};

const EMPTY: Pick<IrisStore, "state" | "brain" | "user" | "segments" | "pending" | "error" | "toolsUsed"> = { state: "idle", brain: null, user: "", segments: [], pending: [], error: null, toolsUsed: [] };

export const useIris = create<IrisStore>()(() => ({ turn: 0, floor: 0, status: null, ...EMPTY }));

/** Demande en route : `cancel` en a besoin pour connaître le numéro du tour. */
let asking: Promise<number> | null = null;
/** `asking` n'a pas encore rendu son numéro de tour. */
let unresolved = false;

export function apply(ev: IrisEvent) {
  useIris.setState((s) => {
    // Tour vocal que le panneau n'a pas demandé : il remplace l'échange comme un nouvel `ask`.
    if (ev.type === "user") return ev.turn > s.floor && ev.turn > s.turn ? { ...EMPTY, user: ev.text, state: "thinking", turn: ev.turn } : s;
    if (ev.turn <= s.floor || ev.turn < s.turn || !s.user) return s;
    const base = { turn: ev.turn };
    switch (ev.type) {
      case "state": return { ...base, state: ev.state, brain: ev.brain };
      case "reply": return { ...base, segments: ev.segments };
      case "tool_used": return { ...base, toolsUsed: s.toolsUsed.includes(ev.tool) ? s.toolsUsed : [...s.toolsUsed, ev.tool] };
      case "pending": return { ...base, pending: [...s.pending, ev.action_id] };
      case "error": return { ...base, error: { kind: ev.kind, message: ev.message, offer_local: ev.offer_local } };
    }
  });
}

/** Envoie un message à IRIS ; le tour précédent est abandonné par Rust. */
export function ask(text: string, forceClaude: boolean, localOnly = false) {
  useIris.setState((s) => ({ ...EMPTY, floor: s.turn, user: text, state: "thinking" }));
  const prev = unresolved ? asking : null;
  const p = irisIpc.ask(text, forceClaude, localOnly);
  asking = p;
  unresolved = true;
  const settle = () => { if (asking === p) unresolved = false; };
  p.then(settle, settle);
  // Le tour abandonné n'a pas encore de numéro : ses événements tardifs passeraient. On le barre dès qu'il est connu.
  prev?.then((t) => useIris.setState((s) => {
    if (s.user !== text) return s;
    return s.turn <= t ? { ...EMPTY, user: text, state: "thinking", turn: t, floor: t } : { floor: Math.max(s.floor, t) };
  }), () => {});
  p.then((t) => useIris.setState((s) => {
    if (s.user !== text || t <= s.turn) return s;
    // Un tour plus ancien (abandonné) a déjà été accepté faute de numéro : son contenu est écarté.
    return s.turn > s.floor ? { ...EMPTY, user: text, state: "thinking", turn: t } : { turn: t };
  }), () => {
    if (asking === p) useIris.setState({ state: "idle", error: { kind: "failed", message: "IRIS n'a pas pu traiter ce message.", offer_local: false } });
  });
}

/** Annule sans attendre : Rust n'émet rien et n'enregistre pas un tour abandonné. */
export function cancel() {
  const p = unresolved ? asking : null;
  const { turn } = useIris.getState();
  useIris.setState((s) => ({ ...EMPTY, floor: s.turn }));
  asking = null;
  if (p) p.then((t) => { void irisIpc.cancel(t); useIris.setState((s) => (s.user === "" && t > s.floor ? { floor: t } : s)); }, () => {});
  // Numéro déjà connu : le tour affiché, tapé ou vocal adopté.
  else if (turn) void irisIpc.cancel(turn);
}

/** Referme l'échange (le briefing revient). */
export const dismiss = () => useIris.setState((s) => ({ ...EMPTY, floor: Math.max(s.turn, s.floor) }));

/** État d'IRIS relu à l'ouverture du panneau seulement. */
export const loadStatus = () => irisIpc.status().then((status) => useIris.setState({ status }), () => {});
