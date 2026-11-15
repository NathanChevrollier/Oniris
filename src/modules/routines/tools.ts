// Outils permis dans une étape de routine : miroir de `oniris_core::routines::ALLOWED` (un test Rust vérifie les libellés).

export type ArgField = {
  key: string;
  title: string;
  kind: "text" | "number" | "bool" | "choice" | "favorite";
  required?: boolean;
  min?: number;
  max?: number;
  placeholder?: string;
  choices?: [string, string][];
};

const app: ArgField = { key: "key", title: "Application", kind: "text", required: true, placeholder: "Steam" };

export const TOOLS: Record<string, { label: string; fields: ArgField[]; confirm?: boolean }> = {
  "apps.launch": { label: "Lancer une application", fields: [app] },
  "apps.focus": { label: "Mettre une application au premier plan", fields: [app] },
  "apps.close": { label: "Fermer une application", fields: [app], confirm: true },
  "pc.volume": { label: "Régler le son du PC", fields: [{ key: "percent", title: "Volume", kind: "number", min: 0, max: 100 }, { key: "muted", title: "Couper le son", kind: "bool" }] },
  "pc.bluetooth": { label: "Activer ou couper le Bluetooth", fields: [{ key: "on", title: "Bluetooth activé", kind: "bool", required: true }] },
  "music.play_playlist": { label: "Lancer une playlist Spotify", fields: [{ key: "name", title: "Playlist", kind: "text", required: true, placeholder: "Focus" }] },
  "music.play_search": { label: "Lancer une musique Spotify", fields: [{ key: "query", title: "Titre, artiste ou playlist", kind: "text", required: true }] },
  "music.toggle": { label: "Lecture ou pause de la musique", fields: [] },
  "music.next": { label: "Titre suivant", fields: [] },
  "music.volume": { label: "Régler le volume de Spotify", fields: [{ key: "percent", title: "Volume", kind: "number", min: 0, max: 100, required: true }] },
  "timer.start": { label: "Lancer un minuteur", fields: [{ key: "text", title: "Minuteur", kind: "text", required: true, placeholder: "25 minutes focus" }] },
  "focus.start": { label: "Lancer une session focus", fields: [] },
  "favorites.open": { label: "Ouvrir un favori", fields: [{ key: "id", title: "Favori", kind: "favorite", required: true }] },
  "projects.resume": { label: "Reprendre un projet", fields: [{ key: "name", title: "Projet", kind: "text", required: true }, { key: "mode", title: "Mode", kind: "choice", required: true, choices: [["normal", "Normal"], ["orchestrator", "Orchestrateur"]] }] },
  "files.open": { label: "Ouvrir un fichier ou un dossier", fields: [{ key: "path", title: "Chemin", kind: "text", required: true, placeholder: "C:\\Projets\\Oniris" }] },
  "notes.create": { label: "Créer une note", fields: [{ key: "text", title: "Texte", kind: "text", required: true }] },
  "reminders.create": { label: "Créer un rappel", fields: [{ key: "text", title: "Rappel", kind: "text", required: true, placeholder: "dans 1 h faire une pause" }] },
};

/** Arguments par défaut d'un outil (choix : la première option ; interrupteur obligatoire : vrai). */
export function defaultArgs(tool: string): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of TOOLS[tool]?.fields ?? []) {
    if (f.kind === "choice" && f.choices) out[f.key] = f.choices[0][0];
    if (f.kind === "bool" && f.required) out[f.key] = true;
  }
  return out;
}

/** Champ obligatoire vide, sinon `null`. */
export function missingField(tool: string, args: Record<string, unknown>): string | null {
  const f = TOOLS[tool]?.fields.find((f) => f.required && (args[f.key] === undefined || args[f.key] === ""));
  return f ? f.title : null;
}
