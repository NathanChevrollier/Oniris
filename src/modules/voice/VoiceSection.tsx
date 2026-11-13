import { useCallback, useEffect, useState } from "react";
import { errorText } from "../accounts/api";
import { Choice, Field, Group, Row, Segmented, Slider, Toggle, useSet } from "../settings/controls";
import { onVoiceInstall, voiceIpc, type VoiceStatus } from "./api";
import "./voice.css";

const mo = (bytes: number) => Math.round(bytes / 1_000_000);

const plural = (n: number, one: string, many: string) => `${n} ${n > 1 ? many : one}`;

/** Compte des extraits du mode apprentissage et leur suppression, avec confirmation. */
function LearnClips() {
  const [count, setCount] = useState<[number, number] | null>(null);
  const [confirm, setConfirm] = useState(false);
  const load = useCallback(() => { voiceIpc.learnCount().then(setCount, () => {}); }, []);
  useEffect(load, [load]);
  return (
    <Row label="Extraits enregistrés" desc="Désactiver le mode n'efface rien : les extraits restent jusqu'à ce que tu les supprimes.">
      {count && <span className="set-chip">{plural(count[0], "positif", "positifs")}, {plural(count[1], "négatif", "négatifs")}</span>}
      {!confirm
        ? <button type="button" className="acc-btn" onClick={() => setConfirm(true)}>Supprimer les extraits</button>
        : <span className="set-ctl" role="alertdialog" aria-label="Confirmer la suppression">
          <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
          <button type="button" className="acc-btn primary" onClick={() => void voiceIpc.learnClear().catch(() => {}).finally(() => { setConfirm(false); load(); })}>Confirmer la suppression</button>
        </span>}
    </Row>
  );
}

/** Voix neurales Microsoft. */
export const AZURE_VOICES: [string, string][] = [
  ["fr-FR-VivienneMultilingualNeural", "Vivienne (femme, très naturelle)"],
  ["fr-FR-DeniseNeural", "Denise (femme)"],
  ["fr-FR-RemyMultilingualNeural", "Rémy (homme, très naturel)"],
  ["fr-FR-HenriNeural", "Henri (homme)"],
  ["fr-FR-EloiseNeural", "Éloïse (jeune femme)"],
];

/** Voix naturelle Azure : réglages, clé dans le coffre Windows (jamais enregistrée seule) et essai. Piper reste le secours hors ligne. */
export function AzureVoice() {
  const { s } = useSet();
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const loadKey = useCallback(() => { voiceIpc.azureKeyStatus().then(setHasKey, () => {}); }, []);
  useEffect(loadKey, [loadKey]);
  const fail = (e: unknown, fallback: string) => setMsg({ ok: false, text: errorText(e, fallback) });
  const saveKey = () => {
    setMsg(null);
    voiceIpc.azureKeySet(key).then(() => { setKey(""); setMsg({ ok: true, text: "Clé enregistrée dans le coffre Windows." }); loadKey(); }, (e) => fail(e, "Clé refusée."));
  };
  const test = () => {
    setMsg(null); setBusy(true);
    voiceIpc.azureTest(s.voice_azure_region.trim(), s.voice_azure_voice).then(() => setMsg({ ok: true, text: "Ça marche !" }), (e) => fail(e, "Essai impossible.")).finally(() => setBusy(false));
  };
  return (
    <Group title="Voix naturelle (Azure)" note="Sans clé ou hors ligne, la voix intégrée prend le relais.">
      <Toggle k="voice_azure" label="Utiliser la voix Azure"
        desc="Les voix neurales de Microsoft, bien plus naturelles que la voix intégrée. Les phrases dites par IRIS sont alors envoyées à Microsoft." />
      <Row label="Clé Azure" id="azure-key" stack desc="La « Clé 1 » de ta ressource Speech. La formule gratuite (F0) couvre 500 000 caractères par mois. La clé est rangée dans le coffre Windows.">
        <form className="set-inline" onSubmit={(e) => { e.preventDefault(); saveKey(); }}>
          <input id="azure-key" type="password" className="set-input wide" aria-label="Clé Azure" aria-describedby="azure-key-d" placeholder={hasKey ? "Remplacer la clé enregistrée" : "Colle ici la Clé 1"}
            value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" spellCheck={false} />
          <button type="submit" className="acc-btn primary" disabled={key.trim() === ""}>Enregistrer la clé</button>
          {hasKey === true && <><span className="set-chip">Enregistrée</span>
            <button type="button" className="acc-btn" onClick={() => void voiceIpc.azureKeyClear().then(loadKey)}>Retirer</button></>}
          {hasKey === false && <span className="set-chip warn">Aucune</span>}
        </form>
      </Row>
      <Field k="voice_azure_region" label="Région" placeholder="francecentral" desc="La région de ta ressource Speech." />
      <Choice k="voice_azure_voice" label="Voix" options={AZURE_VOICES} />
      <Row label="Essai" desc={msg ? <span className={msg.ok ? "set-ok" : "err"} role={msg.ok ? "status" : "alert"}>{msg.text}</span> : "Fais parler la voix choisie, avec ta clé."}>
        <button type="button" className="acc-btn" disabled={busy || !hasKey} onClick={test}>{busy ? "Écoute…" : "Écouter"}</button>
      </Row>
    </Group>
  );
}

/** Catégorie « Voix » : installation sur clic, écoute, réponse, mot d'activation, voix naturelle. */
export function VoiceSection() {
  const { s } = useSet();
  const [wakeState, setWakeState] = useState("");
  const [devices, setDevices] = useState<string[]>([]);
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [file, setFile] = useState<{ key: string; done: number; total: number } | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => { voiceIpc.status().then(setStatus, () => {}); voiceIpc.wakeState().then(setWakeState, () => {}); }, []);
  useEffect(() => { voiceIpc.devices().then(setDevices, () => {}); }, []);
  // L'écoute redémarre après un changement : on relit son état tout de suite, puis une fois lancée.
  useEffect(() => {
    reload();
    const t = window.setTimeout(reload, 1200);
    return () => window.clearTimeout(t);
  }, [reload, s.voice_wake, s.voice_input_device, s.voice_stt_model]);
  useEffect(() => {
    const un = onVoiceInstall((e) => {
      if (e.error) setError(e.error);
      else if (e.key) setFile({ key: e.key, done: e.done ?? 0, total: e.total ?? 0 });
    });
    return () => { void un.then((f) => f()); };
  }, []);
  const install = () => {
    setError(null); setInstalling(true); setFile(null);
    voiceIpc.install().catch((e) => setError(errorText(e, "Installation impossible.")))
      .finally(() => { setInstalling(false); setFile(null); reload(); });
  };
  const pct = file && file.total > 0 ? Math.round((file.done / file.total) * 100) : null;
  return (
    <>
      <Group title="Installation">
        <Row label="Fichiers de la voix" desc="Reconnaissance vocale et voix intégrée, téléchargées une fois puis utilisées sur ton PC.">
          {status?.installed && <span className="set-chip">Installée</span>}
          {status && !status.installed && <>
            <span className="set-chip warn">À installer · {mo(status.bytes_missing)} Mo</span>
            <button type="button" className="acc-btn primary" disabled={installing} onClick={install}>Installer la voix</button></>}
        </Row>
        {(file || error) && (
          <div className="set-row stack">
            {file && <progress className="set-progress" aria-label={`Téléchargement : ${file.key}`} max={file.total || undefined} value={file.total ? file.done : undefined} />}
            {file && <span role="status" className="set-desc">{file.key}{pct !== null ? ` : ${pct} %` : "…"}</span>}
            {error && <p className="err" role="alert">{error}</p>}
          </div>
        )}
      </Group>
      <Group title="Écoute" note="Ta voix reste sur ton PC : rien n'est enregistré ni envoyé.">
        <Segmented k="voice_stt_model" label="Transcription" desc="Le modèle qui transforme ta voix en texte."
          options={[["small", "Rapide"], ["turbo", "Précis · 574 Mo"]]} />
        <Choice k="voice_input_device" label="Micro" options={[["", "Par défaut"], ...devices.map((d): [string, string] => [d, d])]} />
        <Toggle k="voice_follow_up" label="Conversation continue" desc="Après sa réponse, IRIS t'écoute encore quelques secondes." />
        <Field k="voice_unload_minutes" label="Libérer la mémoire après" unit="min" min={1} max={30} desc="La transcription s'arrête après ce temps sans parler." />
      </Group>
      <Group title="Réponse">
        <Toggle k="voice_tts" label="IRIS répond à voix haute" />
        <Slider k="voice_volume" label="Volume" unit=" %" />
      </Group>
      <Group title="Mot d'activation">
        <Toggle k="voice_wake" label="Mot d'activation « Dis Iris »" disabled={!status?.installed}
          desc="Oniris écoute en continu et réagit quand tu dis « Dis Iris » ou « Hé Iris ». Avant ça, rien n'est transcrit, enregistré ni envoyé. Windows affiche l'icône du micro tant que l'écoute est active.">
          {wakeState && <span className="set-chip">{wakeState}</span>}
        </Toggle>
        <Toggle k="voice_wake_learning" label="Mode apprentissage" disabled={!s.voice_wake}
          desc="Garde sur ton PC de courts extraits autour des réveils pour réentraîner « Dis Iris ». Rien n'est envoyé. Les boutons de la pilule permettent de corriger IRIS." />
        <LearnClips />
      </Group>
      <AzureVoice />
    </>
  );
}
