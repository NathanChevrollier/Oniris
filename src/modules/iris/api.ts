import { invoke, isTauri } from "../../lib/ipc";

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
  act: (index: number): Promise<void> => isTauri ? invoke("iris_briefing_act", { index }) : Promise.resolve(),
};
