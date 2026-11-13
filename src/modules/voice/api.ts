import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke, listen } from "../../lib/ipc";
import type { OrbState } from "../../ui/Orb";

export type VoicePhase = "idle" | "listening" | "transcribing" | "speaking";
/** `oniris://voice` : phase du pipeline, niveau du micro (0..1) pendant l'écoute, et `by_wake` quand l'écoute vient du mot d'activation. */
export type VoiceEvent = { phase: VoicePhase; level?: number; by_wake?: boolean };
/** `oniris://voice-install` : progression d'un fichier, erreur ou fin. */
export type InstallEvent = { key?: string; done?: number; total?: number; error?: string; finished?: boolean };
/** Miroir de `oniris_core::voice::assets::VoiceStatus`. */
export type VoiceStatus = { installed: boolean; missing: string[]; bytes_missing: number };

export const voiceIpc = {
  status: (): Promise<VoiceStatus> => invoke("voice_status"),
  install: (): Promise<void> => invoke("voice_install"),
  devices: (): Promise<string[]> => invoke("voice_devices"),
  wakeState: (): Promise<string> => invoke("voice_wake_state"),
  expectWake: (): Promise<void> => invoke("voice_expect_wake"),
  falseWake: (): Promise<void> => invoke("voice_false_wake"),
  /** (positifs, négatifs) */
  learnCount: (): Promise<[number, number]> => invoke("voice_learn_count"),
  learnClear: (): Promise<number> => invoke("voice_learn_clear"),
  /** Clic pour parler : ouvre l'écoute (fermée à la fin de la parole) ou la ferme. */
  tap: (): Promise<void> => invoke("voice_tap"),
};

export const onVoice = (cb: (e: VoiceEvent) => void): Promise<UnlistenFn> => listen<VoiceEvent>("oniris://voice", (e) => cb(e.payload));
export const onVoiceInstall = (cb: (e: InstallEvent) => void): Promise<UnlistenFn> => listen<InstallEvent>("oniris://voice-install", (e) => cb(e.payload));
/** Toast de la pilule (texte brut, jamais du HTML). */
export const onToast = (cb: (text: string) => void): Promise<UnlistenFn> => listen<{ text: string }>("oniris://toast", (e) => cb(e.payload.text));

/** Phase et niveau suivis depuis Rust ; le niveau n'est gardé que pendant l'écoute. */
export function useVoice(): { phase: VoicePhase; level: number } {
  const [v, setV] = useState<{ phase: VoicePhase; level: number }>({ phase: "idle", level: 0 });
  useEffect(() => {
    const un = onVoice((e) => setV({ phase: e.phase, level: e.phase === "listening" ? Math.min(1, Math.max(0, e.level ?? 0)) : 0 }));
    return () => { void un.then((f) => f()); };
  }, []);
  return v;
}

/** État de la sphère : la voix prime sur la réflexion d'IRIS. */
export const orbFor = (phase: VoicePhase, thinking: boolean): OrbState =>
  phase === "listening" ? "listening" : phase === "speaking" ? "speaking" : phase === "transcribing" || thinking ? "thinking" : "idle";
