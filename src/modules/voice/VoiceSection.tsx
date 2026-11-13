import { useCallback, useEffect, useRef, useState } from "react";
import { useSettings, type Settings } from "../../lib/settings";
import { errorText } from "../accounts/api";
import { useSave } from "../../lib/useSave";
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
    <div className="iris-row">
      {count && <span className="acc-state">{plural(count[0], "positif", "positifs")}, {plural(count[1], "négatif", "négatifs")}</span>}
      {!confirm
        ? <button type="button" className="acc-btn" onClick={() => setConfirm(true)}>Supprimer les extraits</button>
        : <span role="alertdialog" aria-label="Confirmer la suppression">
          <button type="button" className="acc-btn primary" onClick={() => void voiceIpc.learnClear().catch(() => {}).finally(() => { setConfirm(false); load(); })}>Confirmer la suppression</button>
          <button type="button" className="acc-btn" onClick={() => setConfirm(false)}>Garder</button>
        </span>}
    </div>
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

/** Voix naturelle Azure : réglages, clé dans le coffre Windows et essai. Piper reste le secours hors ligne. */
function AzureVoice() {
  const s = useSettings();
  const [on, setOn] = useState(s.voice_azure);
  const [region, setRegion] = useState(s.voice_azure_region);
  const [voice, setVoice] = useState(s.voice_azure_voice);
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [save, note] = useSave();
  useEffect(() => { setOn(s.voice_azure); setRegion(s.voice_azure_region); setVoice(s.voice_azure_voice); }, [s.voice_azure, s.voice_azure_region, s.voice_azure_voice]);
  const loadKey = useCallback(() => { voiceIpc.azureKeyStatus().then(setHasKey, () => {}); }, []);
  useEffect(loadKey, [loadKey]);
  const fail = (e: unknown, fallback: string) => setMsg({ ok: false, text: errorText(e, fallback) });
  const saveKey = () => {
    setMsg(null);
    voiceIpc.azureKeySet(key).then(() => { setKey(""); setMsg({ ok: true, text: "Clé enregistrée dans le coffre Windows." }); loadKey(); }, (e) => fail(e, "Clé refusée."));
  };
  const test = () => {
    setMsg(null); setBusy(true);
    voiceIpc.azureTest(region.trim(), voice).then(() => setMsg({ ok: true, text: "Ça marche !" }), (e) => fail(e, "Essai impossible.")).finally(() => setBusy(false));
  };
  return (
    <>
      <h3>Voix naturelle (Azure)</h3>
      <p className="voice-note">Les voix neurales de Microsoft, bien plus naturelles que la voix intégrée. La formule gratuite (F0) couvre 500 000 caractères par mois. Les phrases dites par IRIS sont alors envoyées à Microsoft. Sans clé ou hors ligne, la voix intégrée prend le relais.</p>
      <div className="iris-row">
        <b>Clé Azure</b>
        {hasKey === true && <><span className="acc-state">Enregistrée</span>
          <button type="button" className="acc-btn" onClick={() => void voiceIpc.azureKeyClear().then(loadKey)}>Retirer</button></>}
        {hasKey === false && <span className="acc-state warn">Aucune</span>}
      </div>
      <form className="iris-row" onSubmit={(e) => { e.preventDefault(); saveKey(); }}>
        <input type="password" aria-label="Clé Azure" placeholder="Colle ici la Clé 1 de ta ressource Speech" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" spellCheck={false} />
        <button type="submit" className="acc-btn" disabled={key.trim() === ""}>Enregistrer la clé</button>
      </form>
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save({ voice_azure: on, voice_azure_region: region.trim(), voice_azure_voice: voice }); }}>
        <label>Région<input value={region} onChange={(e) => setRegion(e.target.value)} spellCheck={false} placeholder="francecentral" /></label>
        <label>Voix
          <select value={voice} onChange={(e) => setVoice(e.target.value)}>
            {[...AZURE_VOICES, ...(AZURE_VOICES.some(([v]) => v === voice) ? [] : [[voice, voice] as [string, string]])].map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select></label>
        <label className="voice-check"><input type="checkbox" checked={on} onChange={(e) => setOn(e.target.checked)} />Utiliser la voix Azure</label>
        <button type="button" className="acc-btn" disabled={busy || !hasKey} onClick={test}>{busy ? "Écoute…" : "Écouter"}</button>
        <button type="submit" className="acc-btn primary">Enregistrer la voix naturelle</button>
      </form>
      {msg && <p className={msg.ok ? "voice-note" : "err"} role={msg.ok ? "status" : "alert"}>{msg.text}</p>}
      {note}
    </>
  );
}

/** Section « Voix » de la feuille IRIS : installation sur clic, modèle, volume, micro. */
export function VoiceSection() {
  const s = useSettings();
  const [stt, setStt] = useState(s.voice_stt_model);
  const [tts, setTts] = useState(s.voice_tts);
  const [volume, setVolume] = useState(s.voice_volume);
  const [minutes, setMinutes] = useState(String(s.voice_unload_minutes));
  const [device, setDevice] = useState(s.voice_input_device);
  const [wake, setWake] = useState(s.voice_wake);
  const [learning, setLearning] = useState(s.voice_wake_learning);
  const [followUp, setFollowUp] = useState(s.voice_follow_up);
  const [wakeState, setWakeState] = useState("");
  const [devices, setDevices] = useState<string[]>([]);
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [file, setFile] = useState<{ key: string; done: number; total: number } | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reloadTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(reloadTimer.current), []);
  const [save, note] = useSave();
  useEffect(() => { setStt(s.voice_stt_model); setTts(s.voice_tts); setVolume(s.voice_volume); setMinutes(String(s.voice_unload_minutes)); setDevice(s.voice_input_device); setWake(s.voice_wake); setLearning(s.voice_wake_learning); setFollowUp(s.voice_follow_up); },
    [s.voice_stt_model, s.voice_tts, s.voice_volume, s.voice_unload_minutes, s.voice_input_device, s.voice_wake, s.voice_wake_learning, s.voice_follow_up]);
  const reload = useCallback(() => { voiceIpc.status().then(setStatus, () => {}); voiceIpc.wakeState().then(setWakeState, () => {}); }, []);
  useEffect(() => { reload(); voiceIpc.devices().then(setDevices, () => {}); }, [reload]);
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
  const patch: Partial<Settings> = { voice_stt_model: stt, voice_tts: tts, voice_volume: volume, voice_unload_minutes: Number(minutes), voice_input_device: device, voice_wake: wake, voice_wake_learning: learning, voice_follow_up: followUp };
  return (
    <>
      <h3>Voix</h3>
      <div className="iris-row">
        <b>Fichiers de la voix</b>
        {status?.installed && <span className="acc-state">Installée</span>}
        {status && !status.installed && <>
          <span className="acc-state warn">À installer · {mo(status.bytes_missing)} Mo</span>
          <button type="button" className="acc-btn primary" disabled={installing} onClick={install}>Installer la voix</button></>}
      </div>
      {file && <progress className="iris-progress" aria-label={`Téléchargement : ${file.key}`} max={file.total || undefined} value={file.total ? file.done : undefined} />}
      {file && <span role="status" className="acc-wait">{file.key}{pct !== null ? ` : ${pct} %` : "…"}</span>}
      {error && <p className="err" role="alert">{error}</p>}
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save(patch).then(() => { reload(); window.clearTimeout(reloadTimer.current); reloadTimer.current = window.setTimeout(reload, 1200); }); }}>
        <label>Transcription
          <select value={stt} onChange={(e) => setStt(e.target.value as Settings["voice_stt_model"])}>
            <option value="small">Rapide (small)</option>
            <option value="turbo">Précis (turbo, 574 Mo)</option>
          </select></label>
        <label>Micro
          <select value={device} onChange={(e) => setDevice(e.target.value)}>
            <option value="">Par défaut</option>
            {[...new Set([...devices, ...(device && !devices.includes(device) ? [device] : [])])].map((d) => <option key={d} value={d}>{d}</option>)}
          </select></label>
        <label>Volume ({volume} %)<input type="range" min={0} max={100} value={volume} onChange={(e) => setVolume(Number(e.target.value))} /></label>
        <label>Libération (minutes)<input type="number" min={1} max={30} value={minutes} onChange={(e) => setMinutes(e.target.value)} /></label>
        <label className="voice-check"><input type="checkbox" checked={tts} onChange={(e) => setTts(e.target.checked)} />IRIS répond à voix haute</label>
        <label className="voice-check"><input type="checkbox" checked={wake} disabled={!status?.installed} onChange={(e) => setWake(e.target.checked)} />Mot d'activation « Dis Iris »</label>
        {wakeState && <span className="acc-state">{wakeState}</span>}
        <p className="voice-note">Oniris écoute en continu et réagit quand tu dis « Dis Iris » ou « Hé Iris ». Avant ça, rien n'est transcrit, enregistré ni envoyé. Windows affiche l'icône du micro tant que l'écoute est active.</p>
        <label className="voice-check"><input type="checkbox" checked={learning} disabled={!wake} onChange={(e) => setLearning(e.target.checked)} />Mode apprentissage</label>
        <p className="voice-note">Garde sur ton PC de courts extraits autour des réveils pour réentraîner « Dis Iris ». Rien n'est envoyé. Les boutons de la pilule permettent de corriger IRIS.</p>
        <p className="voice-note">Désactiver le mode n'efface rien : les extraits restent jusqu'à ce que tu les supprimes.</p>
        <LearnClips />
        <label className="voice-check"><input type="checkbox" checked={followUp} onChange={(e) => setFollowUp(e.target.checked)} />Conversation continue : après sa réponse, IRIS t'écoute encore quelques secondes</label>
        <button type="submit" className="acc-btn primary">Enregistrer la voix</button>
      </form>
      {note}
      <p className="voice-note">Ta voix reste sur ton PC : rien n'est enregistré ni envoyé.</p>
      <AzureVoice />
    </>
  );
}
