import { useCallback, useState } from "react";
import type { ToolOutcome } from "./ipc";

/** Erreurs connues des outils (sans le préfixe du courtier), en français lisible. */
const KNOWN: Record<string, string> = {
  "date introuvable": "Date introuvable — essaie « demain 14h dentiste ».",
  "durée introuvable": "Durée introuvable — essaie « 10 min pâtes ».",
  "texte manquant": "Écris quelque chose d'abord.",
  "titre manquant": "Écris quelque chose d'abord.",
  "la fin précède le début": "La fin précède le début.",
  "événement introuvable": "Cet événement n'existe plus.",
  "rappel introuvable": "Ce rappel n'existe plus.",
  "note introuvable": "Cette note n'existe plus.",
  "minuteur introuvable": "Ce minuteur est déjà terminé.",
  "demande expirée": "Demande expirée, recommence.",
  "favori déjà présent": "Ce site est déjà dans tes favoris.",
  "adresse invalide": "Adresse invalide, essaie https://exemple.com.",
  "favori introuvable": "Ce favori n'existe plus.",
  "bientôt": "Bientôt disponible.",
  "chemin hors des dossiers autorisés": "Ce fichier est hors des dossiers de recherche.",
  "application inconnue": "Cette application n'est plus installée.",
};

/** Message affichable pour une erreur brute du courtier (« échec : … », « arguments invalides : … »). */
export function friendlyError(raw: string, fallback: string): string {
  const known = KNOWN[raw.replace(/^(arguments invalides|échec) : /, "")];
  // Envoi masqué dans le journal puis approuvé après un redémarrage : les arguments ne se relisent plus.
  return known ?? (raw.startsWith("arguments invalides") ? "Action expirée, recommence." : fallback);
}

/**
 * Lance une action d'outil depuis une carte. `run` renvoie `true` si l'action est faite ou attend la
 * confirmation de l'utilisateur ; sinon `error` porte un message en français (`unavailable` par défaut).
 */
export function useToolAction(unavailable: string) {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const run = useCallback(async (call: Promise<ToolOutcome>): Promise<boolean> => {
    setPending(true);
    try {
      const out = await call;
      if ("Failed" in out) { setError(friendlyError(out.Failed.error, unavailable)); return false; }
      setError(null);
      return !("Rejected" in out);
    } catch {
      setError(unavailable);
      return false;
    } finally {
      setPending(false);
    }
  }, [unavailable]);
  const clear = useCallback(() => setError(null), []);
  return { run, error, pending, clear };
}
