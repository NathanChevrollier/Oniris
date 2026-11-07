import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Archive, MailCheck, MailPlus, Trash2, WifiOff } from "lucide-react";
import { Card } from "../../ui/Card";
import { Empty } from "../../ui/Empty";
import { useTopic } from "../../lib/data";
import { listen, type ToolOutcome } from "../../lib/ipc";
import { useToolAction } from "../../lib/toolAction";
import { accountsIpc, type AccountView } from "../accounts/api";
import { mailIpc, type Bucket, type MailRow, type MailStatus } from "./api";
import { MailReader } from "./MailReader";
import type { Draft } from "./Composer";
import "./mail.css";

const UNAVAILABLE = "Mails indisponibles, réessaie.";
const NONE_MAILS: MailRow[] = [];
const NONE_ACCOUNTS: AccountView[] = [];
const TABS: { id: Bucket; label: string }[] = [{ id: "todo", label: "À traiter" }, { id: "read", label: "À lire" }, { id: "filtered", label: "Filtrés" }];

/** Première lettre des deux premiers mots. */
const initials = (s: string) => s.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => [...w][0]).join("").toUpperCase() || "?";

/** « 07:58 » aujourd'hui, « hier », sinon « 12 nov. ». */
export function when(ms: number, now = new Date()): string {
  const d = new Date(ms);
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  const y = new Date(now); y.setDate(y.getDate() - 1);
  return d.toDateString() === y.toDateString() ? "hier" : d.toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
}

/** Vrai tant que le navigateur est hors ligne. */
function useOffline(): boolean {
  const [offline, setOffline] = useState(() => typeof navigator !== "undefined" && navigator.onLine === false);
  useEffect(() => {
    const off = () => setOffline(true);
    const on = () => setOffline(false);
    window.addEventListener("offline", off);
    window.addEventListener("online", on);
    return () => { window.removeEventListener("offline", off); window.removeEventListener("online", on); };
  }, []);
  return offline;
}

export function MailCard({ onAccounts }: { onAccounts?: () => void }) {
  const mails = useTopic<MailRow[]>("mail", mailIpc.list, NONE_MAILS);
  const accounts = useTopic<AccountView[]>("accounts", accountsIpc.list, NONE_ACCOUNTS);
  const [tab, setTab] = useState<Bucket>("todo");
  const [opened, setOpened] = useState<MailRow | null>(null);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({}); // en mémoire tant que le panneau est monté
  const { run, error } = useToolAction(UNAVAILABLE);
  const offline = useOffline();
  const status = useTopic<MailStatus | null>("mail", mailIpc.status, null);
  const lastOk = status.data?.last_ok_ms ?? null;
  const gmail = accounts.data.filter((a) => a.provider === "google");
  const reauth = gmail.filter((a) => a.status === "reauth");
  const color = (id: number) => gmail.find((a) => a.id === id)?.color ?? "#8e8e93";
  const unread = (b: Bucket) => mails.data.filter((m) => m.bucket === b && m.unread).length;
  const rows = mails.data.filter((m) => m.bucket === tab);
  const key = opened ? `${opened.account_id}:${opened.id}` : "";

  // « Préparer la réponse » du briefing d'IRIS : la carte vient au premier plan, ouvre le mail et lance le brouillon.
  const [autoDraft, setAutoDraft] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const rowsRef = useRef(mails.data);
  rowsRef.current = mails.data;
  useEffect(() => {
    const un = listen<{ account: number | null; id: string | null; draft: boolean }>("oniris://iris-open-mail", (e) => {
      const { account, id, draft } = e.payload;
      setTab("todo");
      const row = id == null ? undefined : rowsRef.current.find((m) => m.account_id === account && m.id === id);
      if (row) { setOpened(row); setAutoDraft(draft); } else setOpened(null);
      rootRef.current?.scrollIntoView?.({ block: "nearest" });
    });
    return () => { un.then((u) => u()); };
  }, []);

  const act = (call: Promise<ToolOutcome>) => void run(call);

  if (accounts.data !== NONE_ACCOUNTS && gmail.length === 0) {
    return (
      <Card title="Mails" span={5}>
        <Empty icon={MailPlus} tint="#0a84ff">Connecte un compte Gmail</Empty>
        <button type="button" className="mail-btn mail-cta" onClick={onAccounts}>Connecter un compte</button>
      </Card>
    );
  }

  return (
    <Card title="Mails" span={5}
      aside={offline ? <><WifiOff size={11} /> hors ligne{lastOk != null && ` · il y a ${Math.max(1, Math.round((Date.now() - lastOk) / 60_000))} min`}</>
        : gmail.length > 0 ? `${gmail.length} boîte${gmail.length > 1 ? "s" : ""}` : undefined}>
      {reauth.length > 0 && (
        <button type="button" className="mail-reauth" onClick={onAccounts}>Reconnecte {reauth.map((a) => a.label).join(", ")} pour relever ses mails</button>
      )}
      <div className="mail-view" ref={rootRef}>
      {opened ? (
        <MailReader key={key} mail={opened} draft={drafts[key] ?? null} autoDraft={autoDraft} onAutoDraft={() => setAutoDraft(false)} onClose={() => setOpened(null)}
          onDraft={(d) => setDrafts((all) => { const n = { ...all }; if (d) n[key] = d; else delete n[key]; return n; })} />
      ) : (
        <>
          <div className="mail-seg" role="tablist">
            {TABS.map((t) => (
              <button key={t.id} type="button" role="tab" aria-selected={tab === t.id} className={tab === t.id ? "on" : ""} onClick={() => setTab(t.id)}>
                {t.label}<b aria-label={`${unread(t.id)} non lus`}>{unread(t.id)}</b>
              </button>
            ))}
          </div>
          {rows.length === 0 && <Empty icon={MailCheck} tint="#0a84ff">Rien ici pour l'instant.</Empty>}
          <ul className="mail-list">
            {rows.map((m) => {
              const c = color(m.account_id);
              return (
                <li key={`${m.account_id}:${m.id}`} className={`mail-row${m.unread ? " unread" : ""}`}>
                  <button type="button" className="mail-open" onClick={() => setOpened(m)}>
                    <span className="mail-av" style={{ "--c": c } as CSSProperties} data-testid="pastille">{initials(m.from_name || m.from_addr)}</span>
                    <span className="mail-body">
                      <span className="mail-l1"><b>{m.from_name || m.from_addr}</b><time>{when(m.date_ms)}</time></span>
                      <span className="mail-subj">{m.subject}</span>
                      <span className="mail-ai">{m.tag && <span className="mail-tag">{m.tag}</span>}{m.summary ?? m.snippet}</span>
                    </span>
                    {m.unread && <i className="mail-dot" aria-label="Non lu" />}
                  </button>
                  <span className="mail-acts">
                    <button type="button" aria-label="Archiver" title="Archiver" onClick={() => act(mailIpc.archive(m.account_id, m.id))}><Archive size={13} /></button>
                    <button type="button" aria-label="Lu" title="Marquer comme lu" onClick={() => act(mailIpc.setRead(m.account_id, m.id, m.unread))}><MailCheck size={13} /></button>
                    <button type="button" aria-label="Supprimer" title="Supprimer" onClick={() => act(mailIpc.delete(m.account_id, m.id))}><Trash2 size={13} /></button>
                  </span>
                </li>
              );
            })}
          </ul>
        </>
      )}
      </div>
      {error && <p className="err" role="alert">{error}</p>}
    </Card>
  );
}
