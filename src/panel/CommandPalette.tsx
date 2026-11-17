import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { AppWindow, Calculator, FileText, Folder, FolderGit2, Globe, Slash, Star, StickyNote, Workflow, type LucideIcon } from "lucide-react";
import { invoke, ipc, isTauri, listen, type ToolOutcome } from "../lib/ipc";
import { friendlyError, useToolAction } from "../lib/toolAction";
import { focusCard, usePaletteFocus } from "../lib/focusCard";
import { timersIpc } from "../modules/timers/api";
import { notesIpc } from "../modules/notes/api";
import { remindersIpc } from "../modules/reminders/api";
import { agendaIpc } from "../modules/agenda/api";
import { ask, cancel, useIris } from "../modules/iris/store";
import { CommandBar } from "./CommandBar";
import { usePanelStore } from "./store";
import { useSettings, type Settings } from "../lib/settings";
import { calc, convertMoney, convertUnits, parseMoney } from "./calc";

/** Premiers essais : des demandes qu'IRIS sait traiter (agenda, mails, rappels, minuteur, notes, météo). */
export const SUGGESTIONS = ["Qu'est-ce que j'ai aujourd'hui ?", "Résume mes mails importants", "Rappelle-moi de boire de l'eau dans 1 h", "Minuteur 25 minutes", "Note idées pour le week-end", "Quel temps fera-t-il demain ?"];
import "./palette.css";

export type ItemKind = "file" | "dir" | "app" | "project" | "favorite" | "note" | "command" | "routine";
/** Miroirs de `oniris_core::search::Hit` et de `modules::search::SearchResult`. */
export type Hit = { item: { kind: ItemKind; key: string; title: string; subtitle: string }; score: number };
export type SearchResult = { req: number; hits: Hit[]; indexing: boolean };
type Opened = "done" | "pending" | "note" | "command";
type IndexStatus = { state: "indexing"; done: number } | { state: "ready"; count: number };
/** Lignes ajoutées par la barre elle-même : résultat de calcul (en tête) et recherche web (en dernier). */
type Kind = ItemKind | "calc" | "web";
type Row = { item: { kind: Kind; key: string; title: string; subtitle: string }; score: number };
type Rates = Record<string, number> | "loading" | "error" | null;

const OPEN_FAILED = "Impossible d'ouvrir cet élément.";
const DONE: ToolOutcome = { Done: { action_id: 0, output: {} } };
const ICONS: Record<Kind, LucideIcon> = {
  file: FileText, dir: Folder, app: AppWindow, project: FolderGit2, favorite: Star, note: StickyNote, command: Slash, routine: Workflow, calc: Calculator, web: Globe,
};
/** Libellé discret de la sorte, à droite de la ligne (fichiers : le type donné par Windows). */
const KIND_LABEL: Record<Kind, string> = {
  file: "Fichier", dir: "Dossier", app: "Application", project: "Projet", favorite: "Favori", note: "Note", command: "Commande", routine: "Routine", calc: "", web: "",
};

/** Miroir de `modules::icons::Icon`. */
type IconInfo = { png: string | null; type_name: string | null };
const NO_ICON: IconInfo = { png: null, type_name: null };
export const iconIpc = {
  get: (kind: "app" | "file", key: string): Promise<IconInfo> => isTauri ? invoke("search_icon", { kind, key }) : Promise.resolve(NO_ICON),
};
/** Icônes déjà demandées (une requête par application ou par extension, pour toute la vie de la page). */
const iconReqs = new Map<string, Promise<IconInfo>>();
const iconDone = new Map<string, IconInfo>();
const iconKey = (kind: Kind, key: string): string | null => {
  if (kind === "app") return `app:${key}`;
  const ext = kind === "file" ? /\.([a-z0-9]{1,10})$/i.exec(key)?.[1] : undefined;
  return ext ? `ext:${ext.toLowerCase()}` : null;
};

/** Icône réelle chargée après l'affichage (jamais pendant la frappe) ; l'icône lucide de la sorte en attendant. */
function useIcon(kind: Kind, key: string): IconInfo | null {
  const k = iconKey(kind, key);
  const [v, setV] = useState<IconInfo | null>(() => (k && iconDone.get(k)) || null);
  useEffect(() => {
    if (!k) return setV(null);
    const known = iconDone.get(k);
    if (known) return setV(known);
    let alive = true;
    let req = iconReqs.get(k);
    if (!req) {
      req = iconIpc.get(kind as "app" | "file", key).catch(() => NO_ICON).then((i) => { iconDone.set(k, i); return i; });
      iconReqs.set(k, req);
    }
    void req.then((i) => { if (alive) setV(i); });
    return () => { alive = false; };
  }, [k, kind, key]);
  return v;
}

function RowIcon({ kind, path }: { kind: Kind; path: string }) {
  const info = useIcon(kind, path);
  const Icon = ICONS[kind] ?? FileText;
  return (
    <span className={`pal-ico k-${kind}${info?.png ? " real" : ""}`}>
      {info?.png ? <img src={info.png} alt="" width={20} height={20} /> : <Icon size={15} aria-hidden />}
    </span>
  );
}

function KindLabel({ kind, path }: { kind: Kind; path: string }) {
  const info = useIcon(kind, path);
  const ext = kind === "file" ? /\.([a-z0-9]{1,10})$/i.exec(path)?.[1] : undefined;
  const label = info?.type_name ?? (ext ? `Fichier ${ext.toUpperCase()}` : KIND_LABEL[kind] ?? "");
  return label ? <span className="pal-kind">{label}</span> : null;
}
/** Puces de filtre : `kinds` = types envoyés à Rust (« Fichiers » couvre aussi les dossiers) ; « Web » n'a pas de type, c'est la ligne web. */
export const CHIPS = [
  { id: "app", label: "Applis", Icon: AppWindow, kinds: ["app"] },
  { id: "file", label: "Fichiers", Icon: FileText, kinds: ["file", "dir"] },
  { id: "note", label: "Notes", Icon: StickyNote, kinds: ["note"] },
  { id: "project", label: "Projets", Icon: FolderGit2, kinds: ["project"] },
  { id: "favorite", label: "Favoris", Icon: Star, kinds: ["favorite"] },
  { id: "command", label: "Commandes", Icon: Slash, kinds: ["command", "routine"] },
  { id: "web", label: "Web", Icon: Globe, kinds: [] },
] as const satisfies readonly { id: string; label: string; Icon: LucideIcon; kinds: readonly ItemKind[] }[];
export type ChipId = (typeof CHIPS)[number]["id"];
/** Types Rust des puces cochées (sans « Web »). */
const chipKinds = (on: ChipId[]): ItemKind[] => CHIPS.filter((c) => on.includes(c.id)).flatMap((c) => c.kinds as readonly ItemKind[]);

const ENGINES: Record<Settings["web_search_engine"], string> = { google: "Google", duckduckgo: "DuckDuckGo", bing: "Bing" };
const SCOPES: Record<string, ItemKind> = { p: "project", f: "file", a: "app", b: "favorite", n: "note" };

/**
 * Commandes « / » : exécutées ici, par les appels des modules. `arg` : argument attendu (sinon Entrée complète).
 * Miroir des clés dans `COMMANDS` (src-tauri/src/modules/search.rs, vérifié par un test Rust).
 */
type Cmd = { key: string; hint: string; arg?: string; run: (arg: string) => Promise<ToolOutcome> };
export const COMMANDS: Cmd[] = [
  { key: "minuteur", hint: "Lancer un minuteur", arg: "durée", run: (a) => timersIpc.start(a) },
  { key: "focus", hint: "Démarrer une session focus", run: () => timersIpc.focus() },
  { key: "note", hint: "Noter quelque chose", arg: "texte", run: (a) => notesIpc.create(a) },
  { key: "rappel", hint: "Créer un rappel", arg: "texte", run: (a) => remindersIpc.create(a) },
  { key: "événement", hint: "Ajouter un événement à l'agenda", arg: "texte", run: (a) => agendaIpc.create(a) },
  // Réglages relus juste avant : jamais d'écrasement par les valeurs par défaut.
  { key: "météo", hint: "Changer la ville de la météo", arg: "ville",
    run: (city) => ipc.getSettings().then((s) => ipc.saveSettings({ ...s, weather_city: city })).then(() => DONE) },
  { key: "presse-papiers", hint: "Ouvrir l'historique du presse-papiers", run: () => { focusCard("clipboard"); return Promise.resolve(DONE); } },
];

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Préfixe de portée (`p `, `f `, `a `, `b `, `n `, `/`) ôté de la requête. */
export function parseQuery(text: string): { q: string; scope: ItemKind | null } {
  if (text.startsWith("/")) return { q: text.slice(1), scope: "command" };
  const m = /^([pfabn]) (.*)$/s.exec(text);
  return m ? { q: m[2], scope: SCOPES[m[1]] } : { q: text, scope: null };
}

/** Texte replié (sans casse ni accents) caractère par caractère : `at[k]` = indice d'origine du k-ième caractère replié. */
function folded(s: string): { f: string; at: number[]; n: number } {
  const cps = Array.from(s);
  let f = "";
  const at: number[] = [];
  cps.forEach((ch, i) => { const x = fold(ch); f += x; for (let k = 0; k < x.length; k++) at.push(i); });
  return { f, at, n: cps.length };
}

/** Indices d'origine couverts par `len` caractères repliés depuis `start` (marques combinantes comprises). */
function span(m: { at: number[]; n: number }, start: number, len: number): number[] {
  const to = start + len < m.at.length ? m.at[start + len] : m.n;
  return Array.from({ length: to - m.at[start] }, (_, k) => m.at[start] + k);
}

/** Occurrence de `w` en début de mot de préférence, sinon à l'intérieur d'un mot ; -1 si absent. */
function find(f: string, w: string): number {
  const first = f.indexOf(w);
  for (let j = first; j >= 0; j = f.indexOf(w, j + 1)) if (j === 0 || !/[\p{L}\p{N}]/u.test(f[j - 1])) return j;
  return first;
}

/**
 * Indices à surligner (sur la chaîne d'origine, en points de code) : la requête entière si elle y est, sinon chaque
 * mot (début de mot de préférence) s'ils y sont tous, sinon rien. Jamais de sous-séquence éparpillée : elle
 * marquerait des lettres sans rapport (« oni » dans « Réunion client »). Calcul côté interface : nucleo classe
 * aussi sur le chemin complet, ses indices ne correspondraient pas au texte affiché.
 */
export function highlight(text: string, q: string): number[] {
  const words = q.split(/\s+/).filter((w) => w && !w.startsWith("!")).map((w) => fold(w.replace(/[\^$']/g, ""))).filter(Boolean);
  if (!words.length) return [];
  const m = folded(text);
  const whole = words.join(" ");
  const at = m.f.indexOf(whole);
  if (at >= 0) return span(m, at, whole.length);
  const out = new Set<number>();
  for (const w of words) {
    const i = find(m.f, w);
    if (i < 0) return [];
    span(m, i, w.length).forEach((k) => out.add(k));
  }
  return [...out].sort((a, b) => a - b);
}

/** Texte découpé en morceaux, les indices `marks` dans des <mark>. */
function Marked({ text, marks }: { text: string; marks: number[] }) {
  if (!marks.length) return <>{text}</>;
  const on = new Set(marks);
  const runs: { s: string; m: boolean }[] = [];
  Array.from(text).forEach((ch, i) => {
    const last = runs[runs.length - 1];
    if (last && last.m === on.has(i)) last.s += ch; else runs.push({ s: ch, m: on.has(i) });
  });
  return <>{runs.map((r, i) => (r.m ? <mark key={i}>{r.s}</mark> : r.s))}</>;
}

/** Trois derniers segments d'un chemin long : `…\Projets\Oniris\docs`. */
export function shortPath(p: string): string {
  const parts = p.split("\\");
  return parts.length > 4 ? `…\\${parts.slice(-3).join("\\")}` : p;
}

const parentOf = (p: string) => p.slice(0, Math.max(p.lastIndexOf("\\"), 0)) || p;

/** Commandes dont le nom commence par le mot tapé (le nom exact une fois l'argument commencé). */
function commandHits(q: string): Hit[] {
  const sp = q.indexOf(" ");
  const word = fold(sp < 0 ? q : q.slice(0, sp));
  return COMMANDS.filter((c) => (sp < 0 ? fold(c.key).startsWith(word) : fold(c.key) === word)).map((c) => ({
    item: { kind: "command", key: c.key, title: `/${c.key}`, subtitle: c.arg ? `${c.hint} · ${c.arg}` : c.hint }, score: 0,
  }));
}

/** Résultats factices pour l'aperçu navigateur (hors Tauri). */
const FAKE: Hit[] = ([
  ["dir", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris", "Oniris", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets"],
  ["file", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris\\docs\\design\\maquette-panneau.html", "maquette-panneau.html", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris\\docs\\design"],
  ["file", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris\\docs\\superpowers\\specs\\2026-11-03-oniris-design.md", "2026-11-03-oniris-design.md", "C:\\Users\\natha\\Desktop\\Utilitaires\\Projets\\Oniris\\docs\\superpowers\\specs"],
  ["app", "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\OBS Studio.lnk", "OBS Studio", "Application"],
  ["note", "2", "Réunion client jeudi", "Préparer la démo Oniris et les devis"],
  ["command", "focus", "/focus", "Démarrer une session focus"],
  ["file", "C:\\Users\\natha\\Documents\\Factures\\facture-oniris-2026-10.pdf", "facture-oniris-2026-10.pdf", "C:\\Users\\natha\\Documents\\Factures"],
  ["app", "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\Visual Studio Code.lnk", "Visual Studio Code", "Application"],
  ["dir", "C:\\Users\\natha\\Pictures\\Oniris captures", "Oniris captures", "C:\\Users\\natha\\Pictures"],
] as const).map(([kind, key, title, subtitle]) => ({ item: { kind, key, title, subtitle }, score: 0 }));

export const searchIpc = {
  /** `kinds` : types demandés (puces) ; absent ou vide = tout. */
  query: (q: string, scope: ItemKind | null, req: number, kinds?: ItemKind[]): Promise<SearchResult> => isTauri
    ? invoke("search_query", { q, scope, req, kinds })
    : Promise.resolve({ req, indexing: true, hits: (q.trim()
      ? FAKE.filter((h) => (!scope || h.item.kind === scope) && highlight(`${h.item.title} ${h.item.subtitle}`, q).length > 0)
      : FAKE.slice(3, 7)).filter((h) => !kinds?.length || kinds.includes(h.item.kind)) }),
  open: (kind: ItemKind, key: string): Promise<Opened> => isTauri ? invoke("search_open", { kind, key }) : Promise.resolve("done"),
  /** « Reprendre » un projet (VS Code + Claude) ; renvoie un message si Claude doit être lancé à la main. */
  resume: (path: string, mode: "normal" | "orchestrator"): Promise<string | null> =>
    isTauri ? invoke("projects_resume", { path, mode }) : Promise.resolve(null),
};

export const paletteIpc = {
  /** Taux BCE pour 1 EUR (cache Rust de 12 h) ; demandés seulement pour une conversion de devise. */
  rates: (): Promise<Record<string, number>> => isTauri ? invoke("palette_rates") : Promise.resolve({ EUR: 1, USD: 1.08, GBP: 0.85, CHF: 0.94 }),
  /** Ouvre le navigateur sur le moteur réglé ; rien ne part avant ce choix. */
  webSearch: (q: string): Promise<void> => isTauri ? invoke("palette_web_search", { q }) : Promise.resolve(),
};

/** Calcul, conversion d'unités ou de devise en tête ; « Rechercher sur le web » en dernier. Texte libre seulement. */
export function withExtras(text: string, hits: Hit[], rates: Rates, engine: Settings["web_search_engine"], chips: ChipId[] = []): Row[] {
  const { q, scope } = parseQuery(text);
  if (scope !== null || !q.trim()) return hits;
  const money = parseMoney(q);
  const r = chips.length ? null : convertUnits(q) ?? calc(q) ?? (money && rates && typeof rates === "object" ? convertMoney(money, rates) : null);
  const top: Row[] = r ? [{ item: { kind: "calc", key: r.copy, title: r.title, subtitle: r.subtitle }, score: 0 }] : [];
  const web: Row = { item: { kind: "web", key: q.trim(), title: `Rechercher « ${q.trim()} » sur ${ENGINES[engine] ?? "Google"}`, subtitle: "Ouvre ton navigateur" }, score: 0 };
  return [...top, ...hits, ...(!chips.length || chips.includes("web") ? [web] : [])];
}

/** Numéro de requête, croissant pour toute la vie de la page ; seules les requêtes Rust le consomment : la première
 *  vaut toujours 1, ce qui remet le compteur Rust à zéro si la page est rechargée. */
let seq = 0;

/**
 * Barre de commande + recherche universelle : une requête par frappe, sans délai, numérotée (réponses périmées
 * ignorées). Rien n'est cherché à l'ouverture du panneau : les récents se chargent au premier geste dans la barre.
 */
export function CommandPalette() {
  const open = usePanelStore((s) => s.open);
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const exchanging = useIris((s) => s.user !== "");
  const [hits, setHits] = useState<Hit[]>([]);
  /** Les lignes ajoutées (calcul, web) suivent la liste : repliées avec elle. */
  const [shown, setShown] = useState(false);
  const [rates, setRates] = useState<Rates>(null);
  const { web_search_engine: engine, search_filters: filters } = useSettings();
  /** Puces cochées, gardées pendant la session du panneau (vidées à sa fermeture) ; sans le réglage, rien ne filtre. */
  const [picked, setPicked] = useState<ChipId[]>([]);
  const chips = filters ? picked : [];
  const [sel, setSel] = useState(0);
  const [indexing, setIndexing] = useState(false);
  const [indexed, setIndexed] = useState<number | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const { run, error, clear } = useToolAction("Action impossible pour le moment.");
  /** Requête attendue ; 0 = aucune (toute réponse en route est périmée). */
  const latest = useRef(0);
  /** Requête la plus récente pas encore revenue : Entrée attend sa réponse. */
  const inflight = useRef(false);
  const queued = useRef<{ text: string; ctrl: boolean; shift: boolean } | null>(null);

  const reset = () => {
    latest.current = 0;
    inflight.current = false;
    queued.current = null;
    setText(""); setHits([]); setShown(false); setSel(0); setMsg(null); setRates(null); clear();
  };

  const search = (t: string, on: ChipId[] = chips) => {
    setMsg(null); clear();
    queued.current = null; // une Entrée en attente visait l'ancien texte
    const { q, scope } = parseQuery(t);
    setShown(true); setSel(0);
    if (scope === "command") { latest.current = 0; inflight.current = false; queued.current = null; setHits(commandHits(q)); setSel(0); return; }
    const kinds = scope ? [] : chipKinds(on);
    // « Web » seul : rien à chercher côté Rust, seule la ligne web reste.
    if (!scope && on.length && !kinds.length) { latest.current = 0; inflight.current = false; setHits([]); return; }
    const req = ++seq;
    latest.current = req;
    inflight.current = true;
    (kinds.length ? searchIpc.query(q, scope, req, kinds) : searchIpc.query(q, scope, req)).then((r) => {
      if (r.req !== latest.current) return;
      inflight.current = false;
      setHits(r.hits); setSel(0); setIndexing(r.indexing);
      const e = queued.current;
      queued.current = null;
      if (e) submit(e.text, withExtras(e.text, r.hits, rates, engine, on), 0, e.ctrl, e.shift);
    }, () => { if (req === latest.current) { inflight.current = false; queued.current = null; setHits([]); } });
  };

  const edit = (t: string) => { setText(t); search(t); };

  const pick = (next: ChipId[]) => { setPicked(next); if (text.trim() || shown) search(text, next); };
  const toggle = (id: ChipId) => pick(chips.includes(id) ? chips.filter((c) => c !== id) : [...chips, id]);

  const runCommand = async (key: string, t: string) => {
    const c = COMMANDS.find((x) => x.key === key);
    if (!c) return;
    const typed = parseQuery(t);
    const sp = typed.q.indexOf(" ");
    const exact = typed.scope === "command" && fold(sp < 0 ? typed.q : typed.q.slice(0, sp)) === fold(c.key);
    const arg = exact && sp >= 0 ? typed.q.slice(sp + 1).trim() : "";
    if (c.arg && !arg) { edit(`/${c.key} `); input.current?.focus(); return; }
    searchIpc.open("command", c.key).catch(() => {}); // usage seulement (récents)
    if (await run(c.run(arg))) reset();
  };

  const activate = (h: Row, ctrl: boolean, t: string) => {
    const it = h.item;
    if (it.kind === "command") { void runCommand(it.key, t); return; }
    if (it.kind === "calc") {
      navigator.clipboard.writeText(it.key).then(() => setMsg(`${it.key} copié`), () => setMsg("Copie impossible."));
      return;
    }
    if (it.kind === "web") { paletteIpc.webSearch(it.key).then(reset, (e) => setMsg(friendlyError(String(e), OPEN_FAILED))); return; }
    if (ctrl && it.kind === "project") {
      searchIpc.resume(it.key, "normal").then((info) => { if (info) setMsg(info); else reset(); },
        (e) => setMsg(friendlyError(String(e), OPEN_FAILED)));
      return;
    }
    const [kind, key]: [ItemKind, string] = ctrl && (it.kind === "file" || it.kind === "dir") ? ["dir", parentOf(it.key)] : [it.kind, it.key];
    searchIpc.open(kind, key).then((r) => {
      if (r === "note") focusCard("notes", it.title);
      reset(); // « done » : Rust ferme le panneau ; « pending » : la carte de confirmation s'affiche
    }, (e) => setMsg(friendlyError(String(e), OPEN_FAILED)));
  };

  /** Message libre pour IRIS : la barre se vide, la réponse s'affiche dans le panneau. */
  const askIris = (t: string, forceClaude: boolean) => {
    const trimmed = t.trim();
    if (!trimmed || parseQuery(t).scope === "command") return false;
    ask(trimmed, forceClaude);
    reset();
    return true;
  };

  const submit = (t: string, list: Row[], i: number, ctrl: boolean, shift = false) => {
    // Seule ligne, la recherche web ne prend pas Entrée : le texte libre reste pour IRIS (clic pour le web).
    const h = list[i]?.item.kind === "web" && i === 0 && !chips.includes("web") ? undefined : list[i];
    // Ctrl+↵ garde son sens sur fichiers, dossiers, projets et commandes ; ailleurs il interroge IRIS (Ctrl+Maj+↵ : Claude).
    if (h && !(shift || (ctrl && !["file", "dir", "project", "command"].includes(h.item.kind)))) return activate(h, ctrl, t);
    if (!t.trim()) return;
    if (!askIris(t, shift)) setMsg("Commande inconnue — essaie /note, /rappel…");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (filters && e.ctrlKey && !e.altKey && !e.shiftKey && /^[0-7]$/.test(e.key)) {
      e.preventDefault(); // Ctrl+1..7 bascule une puce, Ctrl+0 les efface
      if (e.key === "0") pick([]); else toggle(CHIPS[Number(e.key) - 1].id);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!list.length) { search(text); return; } // récents au premier geste, ou liste repliée par un clic ailleurs
      setSel((s) => Math.min(Math.max(s + (e.key === "ArrowDown" ? 1 : -1), 0), list.length - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (inflight.current) queued.current = { text, ctrl: e.ctrlKey, shift: e.ctrlKey && e.shiftKey };
      else submit(text, list, sel, e.ctrlKey, e.ctrlKey && e.shiftKey); // Maj seul n'a aucun sens ici
    } else if (e.key === "Escape" && useIris.getState().state === "thinking") {
      e.stopPropagation(); // abandonne le tour au lieu de fermer le panneau
      cancel();
    } else if (e.key === "Escape" && (text || msg || error)) {
      e.stopPropagation(); // sinon le panneau se ferme (Panel) : Échap vide d'abord
      reset();
    }
  };

  // Les effets ci-dessous ne dépendent que de leur déclencheur : ils lisent les gestionnaires du rendu courant ici.
  const collapse = () => { latest.current = 0; inflight.current = false; queued.current = null; setHits([]); setShown(false); setMsg(null); clear(); };
  const handlers = useRef({ reset, edit, collapse });
  useEffect(() => { handlers.current = { reset, edit, collapse }; });

  // Ouverture : focus dans la barre, sans recherche ; fermeture : on repart d'une barre vide.
  useEffect(() => { if (open) input.current?.focus(); else { handlers.current.reset(); setPicked([]); } }, [open]);

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!(e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "p")) return;
      e.preventDefault(); // pas d'impression
      input.current?.focus();
      handlers.current.edit("p ");
    };
    window.addEventListener("keydown", onKey);
    const un = listen<IndexStatus>("oniris://index", (e) => setIndexed(e.payload.state === "indexing" ? e.payload.done : null));
    return () => { window.removeEventListener("keydown", onKey); un.then((f) => f()); };
  }, []);

  usePaletteFocus((t) => { input.current?.focus(); edit(t); });

  useEffect(() => { document.getElementById(`pal-opt-${sel}`)?.scrollIntoView?.({ block: "nearest" }); }, [sel, hits, shown]);

  const list = shown ? withExtras(text, hits, rates, engine, chips) : hits;
  const wantsRates = shown && parseQuery(text).scope === null && parseMoney(text) !== null;
  // Taux demandés une fois par ouverture (Rust les garde 12 h) ; un échec n'est pas retenté avant la fermeture.
  useEffect(() => {
    if (!wantsRates || rates !== null) return;
    setRates("loading");
    paletteIpc.rates().then(setRates, () => setRates("error"));
  }, [wantsRates, rates]);

  const { q, scope } = parseQuery(text);
  const hq = scope === "command" ? `/${q.split(" ")[0]}` : q;
  const note = error ?? msg;
  const showHint = indexing && text.trim() !== "" && scope !== "command";
  const visible = list.length > 0 || note != null || showHint;

  // Clic hors de la barre (vide du panneau, carte…) : la liste se replie, le texte reste ; un clic dans la barre la rouvre.
  useEffect(() => {
    if (!visible) return;
    const h = (e: PointerEvent) => { if (!(e.target as Element).closest?.(".palette")) handlers.current.collapse(); };
    document.addEventListener("pointerdown", h);
    return () => document.removeEventListener("pointerdown", h);
  }, [visible]);

  return (
    <div className="palette">
      {/* Zones d'annonce toujours montées (elles doivent exister avant que leur contenu change) ; la fenêtre, elle, n'existe qu'avec du contenu. */}
      <div className="sr-only"><span role="alert">{error}</span><span role="status">{error ? null : msg}</span></div>
      {visible && (
        <div className="pal-pop">
          {list.length > 0 && (
            <ul className="pal-list" id="pal-list" role="listbox" aria-label="Résultats">
              {list.map((h, i) => {
                const path = h.item.kind === "file" || h.item.kind === "dir";
                const sub = path ? shortPath(h.item.subtitle) : h.item.subtitle;
                const tm = highlight(h.item.title, hq);
                const sm = tm.length ? [] : highlight(sub, hq); // correspondance dans le chemin : marquée sur le chemin affiché
                return (
                  <li key={`${h.item.kind}:${h.item.key}`} id={`pal-opt-${i}`} role="option" aria-selected={i === sel}
                    className={`k-${h.item.kind}${i === sel ? " on" : ""}`} onMouseMove={() => setSel(i)}
                    onMouseDown={(e) => e.preventDefault()} onClick={(e) => activate(h, e.ctrlKey, text)}>
                    <RowIcon kind={h.item.kind} path={h.item.key} />
                    <span className="pal-txt">
                      <span className="pal-title"><Marked text={h.item.title} marks={tm} /></span>
                      <span className="pal-sub" title={path ? h.item.subtitle : undefined}><Marked text={sub} marks={sm} /></span>
                    </span>
                    <KindLabel kind={h.item.kind} path={h.item.key} />
                    {i === sel && <kbd className="pal-enter" aria-hidden>↵</kbd>}
                  </li>
                );
              })}
            </ul>
          )}
          <div className="pal-foot" aria-hidden>
            <span className="pal-err">{error}</span>
            <span>{error ? null : msg}</span>
            {note == null && showHint && <span className="pal-idx">indexation…{indexed != null && ` ${indexed.toLocaleString("fr-FR")} fichiers`}</span>}
            {note == null && !showHint && list[sel]?.item.kind === "project" && <span className="pal-tip">Ctrl+↵ reprendre avec Claude</span>}
          </div>
        </div>
      )}
      <CommandBar inputRef={input} value={text} onIris={() => askIris(text, false)} onClaude={() => askIris(text, true)} role="combobox" aria-expanded={list.length > 0} aria-controls={list.length ? "pal-list" : undefined}
        aria-autocomplete="list" aria-activedescendant={list.length ? `pal-opt-${sel}` : undefined}
        onChange={(e) => edit(e.target.value)} onKeyDown={onKeyDown}
        onMouseDown={() => { if (!list.length) search(text); }} />
      {filters && (
        <div className="pal-chips" role="group" aria-label="Filtrer par type">
          {CHIPS.map((c, i) => {
            const on = chips.includes(c.id);
            const n = shown && text.trim() && (on || !chips.length) ? hits.filter((h) => (c.kinds as readonly string[]).includes(h.item.kind)).length : 0;
            return (
              <button key={c.id} type="button" className="pal-chip" aria-pressed={on} title={`${c.label} · Ctrl+${i + 1} · Alt+clic ou double-clic : seulement celle-ci`}
                onMouseDown={(e) => e.preventDefault()} onClick={(e) => (e.altKey ? pick([c.id]) : toggle(c.id))} onDoubleClick={() => pick([c.id])}>
                <c.Icon size={13} aria-hidden />{c.label}{n > 0 && <span className="pal-n" aria-label={`${n} résultats`}>{n}</span>}
              </button>
            );
          })}
        </div>
      )}
      {!text && !exchanging && (
        <div className="pal-try" role="group" aria-label="Exemples de demandes à IRIS">
          <span>Essaie :</span>
          {SUGGESTIONS.map((t) => <button key={t} type="button" className="iris-chip" onClick={() => ask(t, false)}>{t}</button>)}
        </div>
      )}
    </div>
  );
}
