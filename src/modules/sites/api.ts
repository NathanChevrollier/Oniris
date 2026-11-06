import { invoke, ipc, isTauri } from "../../lib/ipc";

/** `offline` : tous les sites échouent à la fois, c'est le PC qui n'a pas Internet. */
export type SiteState = "unknown" | "up" | "slow" | "down" | "offline";

/** Miroir de `oniris_core::sites::SiteView` : `spark` = les 30 dernières mesures réussies (ms). */
export type SiteView = { name: string; url: string; state: SiteState; last_ms: number | null; spark: number[] };

const wave = (base: number, n = 30) => Array.from({ length: n }, (_, i) => Math.round(base + Math.sin(i * 1.3) * base * 0.18 + ((i * 37) % 11) * (base / 60)));

/** Sites factices pour l'aperçu navigateur (hors Tauri) : un lent, un en panne. */
const fakeStore: SiteView[] = [
  { name: "Nexus Dashboard", url: "https://nexus.example.com", state: "up", last_ms: 142, spark: wave(140) },
  { name: "Mon blog", url: "https://blog.example.org", state: "up", last_ms: 96, spark: wave(95) },
  { name: "API Zenytt", url: "https://api.zenytt.example.com", state: "slow", last_ms: 2310, spark: [...wave(300, 22), ...wave(2200, 8)] },
  { name: "Boutique", url: "https://shop.example.net", state: "down", last_ms: null, spark: wave(210, 24) },
  { name: "Documentation", url: "https://docs.example.com", state: "up", last_ms: 188, spark: wave(180) },
];

export const hostOf = (url: string) => { try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return url; } };

export const sitesIpc = {
  list: (): Promise<SiteView[]> => isTauri ? invoke("sites_list") : Promise.resolve([...fakeStore]),
  /** Ajoute un site : le nom par défaut est le domaine. Le Rust valide (http(s), 30 au plus, doublons). */
  add: async (url: string, name?: string): Promise<void> => {
    const u = url.trim();
    const n = name?.trim() || hostOf(u);
    if (!isTauri) { fakeStore.push({ name: n, url: u, state: "unknown", last_ms: null, spark: [] }); return; }
    const { sites } = await ipc.getSettings();
    await invoke("sites_set", { sites: [...sites, { name: n, url: u, slow_ms: 1500 }] });
  },
  remove: async (url: string): Promise<void> => {
    if (!isTauri) { const i = fakeStore.findIndex((s) => s.url === url); if (i >= 0) fakeStore.splice(i, 1); return; }
    const { sites } = await ipc.getSettings();
    await invoke("sites_set", { sites: sites.filter((s) => s.url !== url) });
  },
};

/** Miroir de `oniris_core::sites::VpsStatus` : `zenytt` faux = Zenytt absent, ligne VPS masquée. */
export type VpsView = { server: string; cpu_pct: number; mem_pct: number; disk_pct: number; load1: number; alerts: number; at_ms: number };
export type VpsStatus = { zenytt: boolean; view: VpsView | null; error: string | null };

const fakeVps: VpsStatus = { zenytt: true, view: { server: "vps-ovh", cpu_pct: 23.4, mem_pct: 61.2, disk_pct: 48.7, load1: 0.42, alerts: 0, at_ms: Date.now() }, error: null };

export const vpsIpc = {
  status: (): Promise<VpsStatus> => isTauri ? invoke("vps_status") : Promise.resolve(fakeVps),
  /** Ajoute les domaines HTTPS que Zenytt connaît ; renvoie le nombre de sites ajoutés. */
  importSites: (): Promise<number> => isTauri ? invoke("sites_import_from_zenytt") : Promise.resolve(0),
};

/** « 5/6 en ligne » : lent compte comme en ligne, un site pas encore sondé n'entre pas dans le total. */
export function counter(sites: SiteView[]): { online: number; total: number } {
  const known = sites.filter((s) => s.state !== "unknown");
  return { online: known.filter((s) => s.state === "up" || s.state === "slow").length, total: known.length };
}
