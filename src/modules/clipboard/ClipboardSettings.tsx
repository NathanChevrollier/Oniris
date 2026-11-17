import { useState } from "react";
import { useTopic } from "../../lib/data";
import { errorText } from "../accounts/api";
import { Field, Group, Row, Toggle } from "../settings/controls";
import { clipboardAdmin, type ClipStats } from "./api";

const NO_STATS: ClipStats = { total: 0, pinned: 0 };

/** Réglages → Presse-papiers : réglages enregistrés tout de suite (purge côté Rust), compteur et effacement. */
export function ClipboardSettings() {
  const { data: n, reload } = useTopic<ClipStats>("clipboard", clipboardAdmin.stats, NO_STATS);
  const [confirm, setConfirm] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const clear = () => clipboardAdmin.clear().then(() => { setErr(null); reload(); }, (e) => setErr(errorText(e, "Effacement impossible."))).finally(() => setConfirm(false));
  return (
    <>
      <Group>
        <Toggle k="clipboard_enabled" label="Garder l'historique" desc="Oniris retient ce que tu copies pour te le reproposer." />
        <Toggle k="clipboard_keep_images" label="Garder les images" desc="Désactivé : seuls les textes sont retenus." />
        <Toggle k="clipboard_ignore_sensitive" label="Ignorer les mots de passe et données sensibles" desc="Les copies que les gestionnaires de mots de passe et Windows marquent comme privées ne sont pas gardées." />
        <Field k="clipboard_retention_days" label="Conservation" unit="jours" min={1} max={90} desc="Les éléments plus vieux sont supprimés tout de suite ; les épinglés restent." />
        <Field k="clipboard_max_items" label="Éléments au plus" min={20} max={1000} desc="Les plus anciens non épinglés sont supprimés au-delà." />
      </Group>
      <Group title="Historique actuel">
        <Row label={`${n.total} élément${n.total > 1 ? "s" : ""} gardé${n.total > 1 ? "s" : ""}`} desc={`dont ${n.pinned} épinglé${n.pinned > 1 ? "s" : ""}`}>
          {!confirm && <button type="button" className="acc-btn" disabled={n.total === n.pinned} onClick={() => setConfirm(true)}>Tout effacer (sauf épinglés)</button>}
        </Row>
        {confirm && (
          <div className="set-row set-confirm" role="alertdialog" aria-label="Confirmer l'effacement">
            <p className="set-desc">Supprime définitivement {n.total - n.pinned} élément{n.total - n.pinned > 1 ? "s" : ""}. Les épinglés restent.</p>
            <div className="set-ctl">
              <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
              <button type="button" className="acc-btn primary" onClick={() => void clear()}>Confirmer l'effacement</button>
            </div>
          </div>
        )}
        {err && <div className="set-row"><p className="err" role="alert">{err}</p></div>}
      </Group>
    </>
  );
}
