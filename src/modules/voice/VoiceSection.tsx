import { useCallback, useEffect, useState } from "react";
import { useSettings, type Settings } from "../../lib/settings";
import { errorText } from "../accounts/api";
import { useSave } from "../../lib/useSave";
import { onVoiceInstall, voiceIpc, type VoiceStatus } from "./api";
import "./voice.css";

const mo = (bytes: number) => Math.round(bytes / 1_000_000);

/** Section « Voix » de la feuille IRIS : installation sur clic, modèle, volume, micro. */
export function VoiceSection() {
  const s = useSettings();
  const [stt, setStt] = useState(s.voice_stt_model);
  const [tts, setTts] = useState(s.voice_tts);
  const [volume, setVolume] = useState(s.voice_volume);
  const [minutes, setMinutes] = useState(String(s.voice_unload_minutes));
  const [device, setDevice] = useState(s.voice_input_device);
  const [devices, setDevices] = useState<string[]>([]);
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [file, setFile] = useState<{ key: string; done: number; total: number } | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [save, note] = useSave();
  useEffect(() => { setStt(s.voice_stt_model); setTts(s.voice_tts); setVolume(s.voice_volume); setMinutes(String(s.voice_unload_minutes)); setDevice(s.voice_input_device); },
    [s.voice_stt_model, s.voice_tts, s.voice_volume, s.voice_unload_minutes, s.voice_input_device]);
  const reload = useCallback(() => { voiceIpc.status().then(setStatus, () => {}); }, []);
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
  const patch: Partial<Settings> = { voice_stt_model: stt, voice_tts: tts, voice_volume: volume, voice_unload_minutes: Number(minutes), voice_input_device: device };
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
      <form className="iris-grid" onSubmit={(e) => { e.preventDefault(); void save(patch).then(reload); }}>
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
        <label className="voice-check"><input type="checkbox" disabled />Mot d'activation : bientôt</label>
        <button type="submit" className="acc-btn primary">Enregistrer la voix</button>
      </form>
      {note}
      <p className="voice-note">Ta voix reste sur ton PC : rien n'est enregistré ni envoyé.</p>
    </>
  );
}
