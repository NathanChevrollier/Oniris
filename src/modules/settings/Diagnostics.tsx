import { useCallback, useEffect, useState } from "react";
import { Copy, RefreshCw } from "lucide-react";
import { invoke } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { accountsIpc, type AccountView } from "../accounts/api";
import { irisIpc, type IrisStatus } from "../iris/api";
import { voiceIpc, type VoiceStatus } from "../voice/api";
import { Group, useSet } from "./controls";
import { updateIpc, type UpdateStatus } from "./update";
import type { SettingsCat } from "./SettingsSheet";

type Level = "ok" | "warn" | "bad" | "off";
export type Check = { id: string; label: string; level: Level; text: string; action?: { label: string; run: () => unknown } };
type IndexStatus = { state: "indexing"; done: number } | { state: "ready"; count: number };

/** Ce que lisent les contrôles (commandes déjà existantes, lancées en parallèle) ; `null` : lecture échouée. */
export type Inputs = {
  iris: IrisStatus | null; voice: VoiceStatus | null; mics: string[] | null; wake: string | null; azureKey: boolean | null;
  accounts: AccountView[] | null; update: UpdateStatus | null; index: IndexStatus | null;
};

const settle = <T,>(p: Promise<T>): Promise<T | null> => p.catch(() => null);
export const diagIpc = { index: (): Promise<IndexStatus> => invoke("index_status") };

async function load(): Promise<Inputs> {
  const [iris, voice, mics, wake, azureKey, accounts, update, index] = await Promise.all([
    settle(irisIpc.status()), settle(voiceIpc.status()), settle(voiceIpc.devices()), settle(voiceIpc.wakeState()),
    settle(voiceIpc.azureKeyStatus()), settle(accountsIpc.list()), settle(updateIpc.status()), settle(diagIpc.index()),
  ]);
  return { iris, voice, mics, wake, azureKey, accounts, update, index };
}

const n = (k: number, one: string, many = `${one}s`) => `${k.toLocaleString("fr-FR")} ${k > 1 ? many : one}`;
const hhmm = (ms: number) => new Date(ms).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
const unreadable = (id: string, label: string): Check => ({ id, label, level: "warn", text: "État illisible pour le moment." });

type Acts = { reload: () => void; installOllama: () => void; pullModel: () => void; installVoice: () => void; checkUpdate: () => void;
  enableAutostart: () => void; accounts: () => void; go: (c: SettingsCat) => void };

/** Une ligne par composant ; les textes ne contiennent ni clé, ni adresse e-mail, ni chemin, ni nom de micro. */
export function checks(i: Inputs, s: Settings, a: Acts): Check[] {
  const out: Check[] = [];
  const iris = i.iris;
  out.push(!iris ? unreadable("ollama", "Ollama")
    : !iris.ollama_installed ? { id: "ollama", label: "Ollama", level: "bad", text: "Pas installé : IRIS ne peut pas réfléchir sur ton PC.", action: { label: "Installer", run: a.installOllama } }
      : iris.local === "down" ? { id: "ollama", label: "Ollama", level: "warn", text: "Installé mais ne répond pas ; il se relance au prochain démarrage d'Oniris.", action: { label: "Revérifier", run: a.reload } }
        : iris.local === "no_model" ? { id: "ollama", label: "Ollama", level: "warn", text: `En marche, mais le modèle ${iris.model} n'est pas téléchargé.`, action: { label: "Télécharger", run: a.pullModel } }
          : { id: "ollama", label: "Ollama", level: "ok", text: `En marche, modèle ${iris.model} prêt.` });

  const v = i.voice;
  const mics = i.mics?.length ?? 0;
  out.push(!v ? unreadable("voice", "Voix")
    : !v.installed ? { id: "voice", label: "Voix", level: "warn", text: `Fichiers de la voix à installer (${Math.ceil(v.bytes_missing / 1e6)} Mo).`, action: { label: "Installer", run: a.installVoice } }
      : i.mics && mics === 0 ? { id: "voice", label: "Voix", level: "bad", text: "Fichiers installés, mais aucun micro trouvé.", action: { label: "Ouvrir", run: () => a.go("voice") } }
        : { id: "voice", label: "Voix", level: "ok", text: `Fichiers installés, ${n(mics, "micro trouvé", "micros trouvés")}.` });

  const w = i.wake;
  out.push(!s.voice_wake ? { id: "wake", label: "Mot d'activation", level: "off", text: "Désactivé.", action: { label: "Ouvrir", run: () => a.go("voice") } }
    : w === "à l'écoute" ? { id: "wake", label: "Mot d'activation", level: "ok", text: "À l'écoute de « Dis Iris »." }
      : w === "en pause" ? { id: "wake", label: "Mot d'activation", level: "ok", text: "En pause (jeu, plein écran ou conversation en cours)." }
        : { id: "wake", label: "Mot d'activation", level: w === "désactivé" ? "warn" : "bad", text: `Activé mais ${w ?? "état inconnu"}.`, action: { label: "Ouvrir", run: () => a.go("voice") } });

  out.push(!s.voice_azure ? { id: "azure", label: "Voix Azure", level: "off", text: "Désactivée : la voix locale répond." }
    : i.azureKey ? { id: "azure", label: "Voix Azure", level: "ok", text: "Activée, clé enregistrée dans le coffre Windows." }
      : { id: "azure", label: "Voix Azure", level: "bad", text: "Activée mais aucune clé enregistrée.", action: { label: "Ouvrir", run: () => a.go("voice") } });

  for (const [provider, label] of [["spotify", "Spotify"], ["google", "Comptes Google"]] as const) {
    const list = i.accounts?.filter((x) => x.provider === provider);
    const reauth = list?.filter((x) => x.status === "reauth").length ?? 0;
    const open = { label: "Ouvrir Comptes", run: a.accounts };
    out.push(!list ? unreadable(provider, label)
      : list.length === 0 ? { id: provider, label, level: "off", text: "Aucun compte connecté.", action: open }
        : reauth ? { id: provider, label, level: "warn", text: `${n(reauth, "compte")} à reconnecter sur ${list.length}.`, action: open }
          : { id: provider, label, level: "ok", text: `${n(list.length, "compte connecté", "comptes connectés")}.` });
  }

  const u = i.update;
  const check = { label: "Vérifier", run: a.checkUpdate };
  const when = u?.last_check_ms ? `dernière vérification à ${hhmm(u.last_check_ms)}` : "pas encore vérifiée depuis le lancement";
  out.push(!u ? unreadable("update", "Mises à jour")
    : u.last_error ? { id: "update", label: "Mises à jour", level: "warn", text: `Version ${u.current} ; la dernière vérification a échoué.`, action: check }
      : !s.auto_update ? { id: "update", label: "Mises à jour", level: "warn", text: `Version ${u.current} ; mises à jour automatiques désactivées.`, action: check }
        : u.pending ? { id: "update", label: "Mises à jour", level: "ok", text: `Version ${u.pending[0]} prête, installée à la fermeture.` }
          : { id: "update", label: "Mises à jour", level: "ok", text: `Version ${u.current}, ${when}.`, action: check });

  const ix = i.index;
  out.push(!ix ? unreadable("index", "Index de recherche")
    : ix.state === "ready" ? { id: "index", label: "Index de recherche", level: "ok", text: `${n(ix.count, "élément indexé", "éléments indexés")}.` }
      : { id: "index", label: "Index de recherche", level: "warn", text: `Indexation en cours (${n(ix.done, "élément", "éléments")}).`, action: { label: "Revérifier", run: a.reload } });

  out.push(s.autostart ? { id: "autostart", label: "Démarrage avec Windows", level: "ok", text: "Oniris se lance à l'ouverture de ta session." }
    : { id: "autostart", label: "Démarrage avec Windows", level: "warn", text: "Oniris ne se lance pas tout seul avec Windows.", action: { label: "Activer", run: a.enableAutostart } });
  return out;
}

const WORD: Record<Level, string> = { ok: "OK", warn: "À surveiller", bad: "Problème", off: "Désactivé" };

/** Rapport texte, sans donnée personnelle (mêmes textes que l'écran). */
export const report = (list: Check[], version: string | null) =>
  [`Diagnostic Oniris${version ? ` ${version}` : ""} — ${new Date().toISOString().slice(0, 16).replace("T", " ")}`,
    ...list.map((c) => `- ${c.label} : ${WORD[c.level]} — ${c.text}`)].join("\n");

export function Diagnostics({ onAccounts, go }: { onAccounts: () => void; go: (c: SettingsCat) => void }) {
  const { s, save } = useSet();
  const [inputs, setInputs] = useState<Inputs | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const reload = useCallback(() => { void load().then(setInputs); }, []);
  useEffect(reload, [reload]);
  // Action longue (installation, téléchargement) : bouton occupé, puis relecture.
  const act = (id: string, p: () => Promise<unknown>) => () => { setBusy(id); void p().catch(() => {}).finally(() => { setBusy(null); reload(); }); };
  const list = inputs && checks(inputs, s, {
    reload, go, accounts: onAccounts,
    installOllama: act("ollama", irisIpc.installOllama), pullModel: act("ollama", irisIpc.pullModel), installVoice: act("voice", voiceIpc.install),
    checkUpdate: act("update", updateIpc.checkNow), enableAutostart: act("autostart", () => save({ autostart: true })),
  });
  const copy = () => {
    if (!list) return;
    navigator.clipboard.writeText(report(list, inputs?.update?.current ?? null)).then(() => setCopied("Rapport copié."), () => setCopied("Copie impossible."));
  };
  return (
    <Group note={copied ? <span role="status">{copied}</span> : "Le rapport ne contient ni clé, ni adresse e-mail, ni chemin de fichier."}>
      {!list ? <div className="set-row"><span className="set-desc" role="status">Vérification…</span></div> : list.map((c) => (
        <div key={c.id} className="set-row diag-row" data-level={c.level}>
          <span className="diag-dot" aria-hidden />
          <div className="set-txt"><span className="set-label">{c.label}</span><p className="set-desc">{c.text}</p></div>
          <span className="sr-only">{WORD[c.level]}</span>
          {c.action && <div className="set-ctl"><button type="button" className="acc-btn" disabled={busy === c.id} onClick={c.action.run}>
            {busy === c.id ? "En cours…" : c.action.label}</button></div>}
        </div>
      ))}
      <div className="set-row diag-tools">
        <button type="button" className="acc-btn" onClick={reload}><RefreshCw size={13} />Revérifier</button>
        <button type="button" className="acc-btn primary" disabled={!list} onClick={copy}><Copy size={13} />Copier le rapport</button>
      </div>
    </Group>
  );
}
