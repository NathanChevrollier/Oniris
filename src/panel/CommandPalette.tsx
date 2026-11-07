import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { AppWindow, FileText, Folder, FolderGit2, Slash, Star, StickyNote, type LucideIcon } from "lucide-react";
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
import "./palette.css";

export type ItemKind = "file" | "dir" | "app" | "project" | "favorite" | "note" | "command";
/** Miroirs de `oniris_core::search::Hit` et de `modules::search::SearchResult`. */
export type Hit = { item: { kind: ItemKind; key: string; title: string; subtitle: string }; score: number };
export type SearchResult = { req: number; hits: Hit[]; indexing: boolean };
type Opened = "done" | "pending" | "note" | "command";
type IndexStatus = { state: "indexing"; done: number } | { state: "ready"; count: number };

const OPEN_FAILED = "Impossible d'ouvrir cet élément.";
const DONE: ToolOutcome = { Done: { action_id: 0, output: {} } };
const ICONS: Record<ItemKind, LucideIcon> = {
  file: FileText, dir: Folder, app: AppWindow, project: FolderGit2, favorite: Star, note: StickyNote, command: Slash,
};
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
  query: (q: string, scope: ItemKind | null, req: number): Promise<SearchResult> => isTauri
    ? invoke("search_query", { q, scope, req })
    : Promise.resolve({ req, indexing: true, hits: q.trim()
      ? FAKE.filter((h) => (!scope || h.item.kind === scope) && highlight(`${h.item.title} ${h.item.subtitle}`, q).length > 0)
      : FAKE.slice(3, 7) }),
  open: (kind: ItemKind, key: string): Promise<Opened> => isTauri ? invoke("search_open", { kind, key }) : Promise.resolve("done"),
  /** « Reprendre » un projet (VS Code + Claude) ; renvoie un message si Claude doit être lancé à la main. */
  resume: (path: string, mode: "normal" | "orchestrator"): Promise<string | null> =>
    isTauri ? invoke("projects_resume", { path, mode }) : Promise.resolve(null),
};

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
  const [hits, setHits] = useState<Hit[]>([]);
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
    setText(""); setHits([]); setSel(0); setMsg(null); clear();
  };

  const search = (t: string) => {
    setMsg(null); clear();
    queued.current = null; // une Entrée en attente visait l'ancien texte
    const { q, scope } = parseQuery(t);
    if (scope === "command") { latest.current = 0; inflight.current = false; queued.current = null; setHits(commandHits(q)); setSel(0); return; }
    const req = ++seq;
    latest.current = req;
    inflight.current = true;
    searchIpc.query(q, scope, req).then((r) => {
      if (r.req !== latest.current) return;
      inflight.current = false;
      setHits(r.hits); setSel(0); setIndexing(r.indexing);
      const e = queued.current;
      queued.current = null;
      if (e) submit(e.text, r.hits, 0, e.ctrl, e.shift);
    }, () => { if (req === latest.current) { inflight.current = false; queued.current = null; setHits([]); } });
  };

  const edit = (t: string) => { setText(t); search(t); };

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

  const activate = (h: Hit, ctrl: boolean, t: string) => {
    const it = h.item;
    if (it.kind === "command") { void runCommand(it.key, t); return; }
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

  const submit = (t: string, list: Hit[], i: number, ctrl: boolean, shift = false) => {
    const h = list[i];
    // Ctrl+↵ garde son sens sur fichiers, dossiers et projets ; ailleurs il interroge IRIS (Ctrl+Maj+↵ : Claude).
    if (h && !(shift || (ctrl && !["file", "dir", "project"].includes(h.item.kind)))) return activate(h, ctrl, t);
    if (!t.trim()) return;
    if (!askIris(t, shift)) setMsg("Commande inconnue — essaie /note, /rappel…");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!hits.length) { if (!text) search(""); return; } // récents, au premier geste seulement
      setSel((s) => Math.min(Math.max(s + (e.key === "ArrowDown" ? 1 : -1), 0), hits.length - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (inflight.current) queued.current = { text, ctrl: e.ctrlKey, shift: e.shiftKey };
      else submit(text, hits, sel, e.ctrlKey, e.shiftKey);
    } else if (e.key === "Escape" && useIris.getState().state === "thinking") {
      e.stopPropagation(); // abandonne le tour au lieu de fermer le panneau
      cancel();
    } else if (e.key === "Escape" && (text || msg || error)) {
      e.stopPropagation(); // sinon le panneau se ferme (Panel) : Échap vide d'abord
      reset();
    }
  };

  // Ouverture : focus dans la barre, sans recherche ; fermeture : on repart d'une barre vide.
  useEffect(() => { if (open) input.current?.focus(); else reset(); }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!(e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "p")) return;
      e.preventDefault(); // pas d'impression
      input.current?.focus();
      edit("p ");
    };
    window.addEventListener("keydown", onKey);
    const un = listen<IndexStatus>("oniris://index", (e) => setIndexed(e.payload.state === "indexing" ? e.payload.done : null));
    return () => { window.removeEventListener("keydown", onKey); un.then((f) => f()); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  usePaletteFocus((t) => { input.current?.focus(); edit(t); });

  useEffect(() => { document.getElementById(`pal-opt-${sel}`)?.scrollIntoView?.({ block: "nearest" }); }, [sel, hits]);

  const { q, scope } = parseQuery(text);
  const hq = scope === "command" ? `/${q.split(" ")[0]}` : q;
  const note = error ?? msg;
  const showHint = indexing && text.trim() !== "" && scope !== "command";
  const visible = hits.length > 0 || note != null || showHint;

  return (
    <div className="palette">
      {/* Toujours monté : les zones d'annonce (status, alert) doivent exister avant que leur contenu change. */}
      <div className={visible ? "pal-pop show" : "pal-pop"}>
          {hits.length > 0 && (
            <ul className="pal-list" id="pal-list" role="listbox" aria-label="Résultats">
              {hits.map((h, i) => {
                const Icon = ICONS[h.item.kind];
                const path = h.item.kind === "file" || h.item.kind === "dir";
                const sub = path ? shortPath(h.item.subtitle) : h.item.subtitle;
                const tm = highlight(h.item.title, hq);
                const sm = tm.length ? [] : highlight(sub, hq); // correspondance dans le chemin : marquée sur le chemin affiché
                return (
                  <li key={`${h.item.kind}:${h.item.key}`} id={`pal-opt-${i}`} role="option" aria-selected={i === sel}
                    className={i === sel ? "on" : undefined} onMouseMove={() => setSel(i)}
                    onMouseDown={(e) => e.preventDefault()} onClick={(e) => activate(h, e.ctrlKey, text)}>
                    <span className={`pal-ico k-${h.item.kind}`}><Icon size={15} aria-hidden /></span>
                    <span className="pal-txt">
                      <span className="pal-title"><Marked text={h.item.title} marks={tm} /></span>
                      <span className="pal-sub" title={path ? h.item.subtitle : undefined}><Marked text={sub} marks={sm} /></span>
                    </span>
                    {i === sel && <kbd className="pal-enter" aria-hidden>↵</kbd>}
                  </li>
                );
              })}
            </ul>
          )}
          <div className="pal-foot">
            <span role="alert">{error}</span>
            <span role="status">{error ? null : msg}</span>
            {note == null && showHint && <span className="pal-idx">indexation…{indexed != null && ` ${indexed.toLocaleString("fr-FR")} fichiers`}</span>}
            {note == null && !showHint && hits[sel]?.item.kind === "project" && <span className="pal-tip">Ctrl+↵ reprendre avec Claude</span>}
          </div>
      </div>
      <CommandBar inputRef={input} value={text} onClaude={() => askIris(text, true)} role="combobox" aria-expanded={hits.length > 0} aria-controls={hits.length ? "pal-list" : undefined}
        aria-autocomplete="list" aria-activedescendant={hits.length ? `pal-opt-${sel}` : undefined}
        onChange={(e) => edit(e.target.value)} onKeyDown={onKeyDown}
        onMouseDown={() => { if (!text && !hits.length) search(""); }} />
    </div>
  );
}
