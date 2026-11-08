import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, isTauri, listen } from "../../lib/ipc";

/** Miroir de `oniris_core::iris::Segment` : le texte d'un tiers garde sa source. */
export type Segment = { t: "Text"; text: string } | { t: "Untrusted"; text: string; source: string };

/** Miroir de `oniris_core::iris::briefing::Suggestion` (le libellé est fixé par le code). */
export type Suggestion = { kind: "reply_mail" | "resume_project" | "open_todo" | "start_focus"; label: string };

export type BriefingData = { segments: Segment[]; suggestions: Suggestion[]; at_ms: number };

const fake = (): BriefingData => ({
  segments: [
    { t: "Text", text: "3 mails à traiter. Prochain : " }, { t: "Untrusted", text: "Point budget avec la banque", source: "agenda externe" },
    { t: "Text", text: " à 14 h. Sur MacroWorld, tu en es à l'étape 7 sur 12." },
  ],
  suggestions: [
    { kind: "reply_mail", label: "Préparer la réponse à Marc Dupont" },
    { kind: "resume_project", label: "Reprendre MacroWorld · orchestrateur" },
    { kind: "start_focus", label: "Lancer un focus" },
  ],
  at_ms: Date.now(),
});

export const irisBriefingIpc = {
  /** Briefing en mémoire (aucun calcul) ; `null` tant qu'aucun n'existe. */
  get: (): Promise<BriefingData | null> => isTauri ? invoke("iris_briefing") : Promise.resolve(fake()),
  /** `atMs` : le briefing affiché ; refusé (`briefing_stale`) s'il a été remplacé. */
  act: (index: number, atMs: number): Promise<void> => isTauri ? invoke("iris_briefing_act", { index, atMs }) : Promise.resolve(),
};

export type Brain = "fast" | "local" | "claude";
/** Miroir de `modules::iris::IrisStatus` (`local` en snake_case, `claude` étiqueté par `kind`). */
export type IrisStatus = { local: "down" | "no_model" | "ready"; model: string; claude: { kind: "found"; path: string } | { kind: "missing" } };
export type IrisErrorKind = "claude_unavailable" | "claude_missing" | "local_unavailable" | "failed";
/** Miroir de `IrisEvent` / `IrisEventKind` (étiquette `type`) sur `oniris://iris`. */
export type IrisEvent = { turn: number } & (
  | { type: "state"; state: "idle" | "thinking"; brain: Brain }
  | { type: "reply"; segments: Segment[]; done: boolean }
  | { type: "tool_used"; tool: string }
  | { type: "pending"; action_id: number }
  | { type: "error"; kind: IrisErrorKind; message: string; offer_local: boolean }
);
export type HistoryItem = { conversation_id: number; ts_ms: number; role: string; shown_text: string };
export type Memory = { id: number; text: string; created_ms: number };

export const irisIpc = {
  /** `localOnly` : « Version simple en local », jamais relayé à Claude. */
  ask: (text: string, forceClaude: boolean, localOnly = false): Promise<number> => invoke("iris_ask", { text, forceClaude, localOnly }),
  cancel: (turn: number): Promise<void> => invoke("iris_cancel", { turn }),
  status: (): Promise<IrisStatus> => invoke("iris_status"),
  history: (limit: number): Promise<HistoryItem[]> => invoke("iris_history", { limit }),
  historyClear: (): Promise<void> => invoke("iris_history_clear"),
  memories: (): Promise<Memory[]> => invoke("iris_memories"),
  memoryDelete: (id: number): Promise<unknown> => invoke("iris_memory_delete", { id }),
  conversationNew: (): Promise<number> => invoke("iris_conversation_new"),
  draftReply: (account: number, id: string): Promise<string> => invoke("iris_draft_reply", { account, id }),
  draftReplyLocal: (account: number, id: string): Promise<string> => invoke("iris_draft_reply_local", { account, id }),
  pullModel: (): Promise<void> => invoke("iris_pull_model"),
};

export const onIris = (cb: (e: IrisEvent) => void): Promise<UnlistenFn> => listen<IrisEvent>("oniris://iris", (e) => cb(e.payload));

/** Progression de `iris_pull_model` sur `oniris://iris-pull` : octets, ou une erreur. */
export type PullEvent = { done: number | null; total: number | null; error: string | null };
export const onIrisPull = (cb: (e: PullEvent) => void): Promise<UnlistenFn> => listen<PullEvent>("oniris://iris-pull", (e) => cb(e.payload));
