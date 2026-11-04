import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { AppWindow, FileText, Folder, FolderGit2, Slash, Star, StickyNote, type LucideIcon } from "lucide-react";
import { invoke, ipc, isTauri, listen, type ToolOutcome } from "../lib/ipc";
import { friendlyError, useToolAction } from "../lib/toolAction";
import { focusCard } from "../lib/focusCard";
import { timersIpc } from "../modules/timers/api";
import { notesIpc } from "../modules/notes/api";
import { remindersIpc } from "../modules/reminders/api";
import { agendaIpc } from "../modules/agenda/api";
import { CommandBar } from "./CommandBar";
import { usePanelStore } from "./store";
import "./palette.css";

export type ItemKind = "file" | "dir" | "app" | "project" | "favorite" | "note" | "command";
/** Miroirs de `oniris_core::search::Hit` et de `modules::search::SearchResult`. */
export type Hit = { item: { kind: ItemKind; key: string; title: string; subtitle: string }; score: number };
export type SearchResult = { req: number; hits: Hit[]; indexing: boolean };
type Opened = "done" | "pending" | "note" | "command";
type IndexStatus = { state: "indexing"; done: number } | { state: "ready"; count: number };

const IRIS_SOON = "IRIS arrive bientôt — essaie /note, /rappel…";
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

/**
 * Caractères à surligner : chaque mot de la requête cherché dans le titre (sans casse ni accents) ; un mot qui n'y est pas entier ne surligne rien. Calcul côté interface : le classement nucleo
 * porte aussi sur le chemin, ses indices ne correspondraient pas toujours au titre affiché.
 */
export function highlight(title: string, q: string): number[] {
  const chars = Array.from(title).map(fold);
  const out = new Set<number>();
  for (const w of q.split(/\s+/).filter((w) => w && !w.startsWith("!"))) {
    const want = Array.from(fold(w.replace(/[\^$']/g, "")));
    // Sous-chaîne d'abord (surlignage d'un seul tenant), sinon sous-séquence gloutonne.
    const at = chars.findIndex((_, k) => want.every((c, j) => chars[k + j] === c));
    const got: number[] = at >= 0 ? want.map((_, j) => at + j) : [];
    for (let i = 0; at < 0 && i < chars.length && got.length < want.length; i++) if (chars[i] === want[got.length]) got.push(i);
    if (got.length === want.length) got.forEach((i) => out.add(i));
  }
  return [...out].sort((a, b) => a - b);
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
};

/** Numéro de requête, croissant pour toute la vie de la page ; seules les requêtes Rust le consomment : la première
 *  vaut toujours 1, ce qui remet le compteur Rust à zéro si la page est rechargée. */
let seq = 0;

/**
 * Barre de commande + recherche universelle : une requête par frappe, sans délai, numérotée (réponses périmées
 * ignorées). Rien n'est cherché à l'ouverture du panneau : les récents se chargent au premier geste dans la barre.
 */
export function CommandPalette({ onCommand = () => {} }: { onCommand?: (text: string) => void }) {
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
  const queued = useRef<{ text: string; ctrl: boolean } | null>(null);

  const reset = () => {
    latest.current = 0;
    inflight.current = false;
    queued.current = null;
    setText(""); setHits([]); setSel(0); setMsg(null); clear();
  };

  const search = (t: string) => {
    setMsg(null); clear();
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
      if (e) submit(e.text, r.hits, 0, e.ctrl);
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
    const [kind, key]: [ItemKind, string] = ctrl && (it.kind === "file" || it.kind === "dir") ? ["dir", parentOf(it.key)] : [it.kind, it.key];
    searchIpc.open(kind, key).then((r) => {
      if (r === "note") focusCard("notes", it.title);
      reset(); // « done » : Rust ferme le panneau ; « pending » : la carte de confirmation s'affiche
    }, (e) => setMsg(friendlyError(String(e), OPEN_FAILED)));
  };

  const submit = (t: string, list: Hit[], i: number, ctrl: boolean) => {
    if (list[i]) return activate(list[i], ctrl, t);
    const trimmed = t.trim();
    if (!trimmed) return;
    if (parseQuery(t).scope === "command") { setMsg("Commande inconnue — essaie /note, /rappel…"); return; }
    onCommand(trimmed);
    setText(""); setHits([]);
    setMsg(IRIS_SOON);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!hits.length) { if (!text) search(""); return; } // récents, au premier geste seulement
      setSel((s) => Math.min(Math.max(s + (e.key === "ArrowDown" ? 1 : -1), 0), hits.length - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (inflight.current) queued.current = { text, ctrl: e.ctrlKey };
      else submit(text, hits, sel, e.ctrlKey);
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

  useEffect(() => { document.getElementById(`pal-opt-${sel}`)?.scrollIntoView?.({ block: "nearest" }); }, [sel, hits]);

  const { q, scope } = parseQuery(text);
  const hq = scope === "command" ? `/${q.split(" ")[0]}` : q;
  const note = error ?? msg;
  const showHint = indexing && text.trim() !== "" && scope !== "command";

  return (
    <div className="palette">
      {(hits.length > 0 || note || showHint) && (
        <div className="pal-pop">
          {hits.length > 0 && (
            <ul className="pal-list" id="pal-list" role="listbox" aria-label="Résultats">
              {hits.map((h, i) => {
                const Icon = ICONS[h.item.kind];
                const marks = new Set(highlight(h.item.title, hq));
                const path = h.item.kind === "file" || h.item.kind === "dir";
                return (
                  <li key={`${h.item.kind}:${h.item.key}`} id={`pal-opt-${i}`} role="option" aria-selected={i === sel}
                    className={i === sel ? "on" : undefined} onMouseMove={() => setSel(i)}
                    onMouseDown={(e) => e.preventDefault()} onClick={(e) => activate(h, e.ctrlKey, text)}>
                    <span className={`pal-ico k-${h.item.kind}`}><Icon size={15} aria-hidden /></span>
                    <span className="pal-txt">
                      <span className="pal-title">{Array.from(h.item.title).map((ch, j) => marks.has(j) ? <mark key={j}>{ch}</mark> : ch)}</span>
                      <span className="pal-sub" title={path ? h.item.subtitle : undefined}>{path ? shortPath(h.item.subtitle) : h.item.subtitle}</span>
                    </span>
                    {i === sel && <kbd className="pal-enter" aria-hidden>↵</kbd>}
                  </li>
                );
              })}
            </ul>
          )}
          {(note || showHint) && (
            <p className="pal-foot">
              {note ? <span role={error ? "alert" : "status"}>{note}</span>
                : <span className="pal-idx">indexation…{indexed != null && ` ${indexed.toLocaleString("fr-FR")} fichiers`}</span>}
            </p>
          )}
        </div>
      )}
      <CommandBar inputRef={input} value={text} role="combobox" aria-expanded={hits.length > 0} aria-controls={hits.length ? "pal-list" : undefined}
        aria-autocomplete="list" aria-activedescendant={hits.length ? `pal-opt-${sel}` : undefined}
        onChange={(e) => edit(e.target.value)} onKeyDown={onKeyDown}
        onMouseDown={() => { if (!text && !hits.length) search(""); }} />
    </div>
  );
}
