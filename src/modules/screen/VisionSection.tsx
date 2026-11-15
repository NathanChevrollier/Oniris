import { useCallback, useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "../../lib/ipc";
import { errorText } from "../accounts/api";
import { Field, Group, Row, useSet } from "../settings/controls";

// Réglages → IRIS : modèle de vision local de « Qu'est-ce qu'il y a sur mon écran ? ».

type Status = "down" | "no_model" | "ready";
type PullEvent = { done: number | null; total: number | null; error: string | null };

export const visionIpc = {
  status: (): Promise<[Status, string]> => invoke("screen_vision_status"),
  pull: (): Promise<void> => invoke("screen_vision_pull"),
};
const onVisionPull = (cb: (e: PullEvent) => void): Promise<UnlistenFn> => listen<PullEvent>("oniris://vision-pull", (e) => cb(e.payload));

export function VisionSection() {
  const { s } = useSet();
  const [status, setStatus] = useState<Status | null>(null);
  const reload = useCallback(() => { visionIpc.status().then(([st]) => setStatus(st), () => {}); }, []);
  useEffect(reload, [reload, s.iris_vision_model]);
  const [pull, setPull] = useState<{ done: number; total: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const un = onVisionPull((e) => { if (e.error) setErr(e.error); else setPull({ done: e.done ?? 0, total: e.total ?? 0 }); });
    return () => { void un.then((f) => f()); };
  }, []);
  const download = () => {
    setErr(null); setPull({ done: 0, total: 0 });
    visionIpc.pull().catch((e) => setErr(errorText(e, "Téléchargement impossible."))).finally(() => { setPull(null); reload(); });
  };
  const pct = pull && pull.total > 0 ? Math.round((pull.done / pull.total) * 100) : null;
  return (
    <Group title="Vision de l'écran" note="Quand tu demandes « qu'est-ce qu'il y a sur mon écran ? », IRIS prend une capture de l'écran principal (sans les fenêtres d'Oniris), la garde en mémoire et la fait lire par ce modèle sur ton PC. Elle n'est jamais enregistrée ni envoyée à Claude, et pas pendant un jeu en plein écran sauf si tu le demandes.">
      <Row label="Modèle de vision" desc="Environ 6 Go à télécharger une fois.">
        {status === "ready" && <span className="set-chip">Installé</span>}
        {status === "down" && <span className="set-chip warn">Ollama ne répond pas</span>}
        {status === "no_model" && <button type="button" className="acc-btn primary" disabled={pull !== null} onClick={download}>Télécharger le modèle de vision</button>}
      </Row>
      {(pull || err) && (
        <div className="set-row stack">
          {pull && <progress className="set-progress" aria-label="Téléchargement du modèle de vision" max={pull.total || undefined} value={pull.total ? pull.done : undefined} />}
          {pull && <span role="status" className="set-desc">Modèle de vision{pct !== null ? ` : ${pct} %` : "…"}</span>}
          {err && <p className="err" role="alert">{err}</p>}
        </div>
      )}
      <Field k="iris_vision_model" label="Nom du modèle" desc="Modèle Ollama capable de lire une image, par exemple qwen3-vl:8b ou qwen2.5vl:7b." />
    </Group>
  );
}
