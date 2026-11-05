import { useEffect, useState } from "react";
import { Archive, ArrowLeft, ExternalLink, ImageIcon, Paperclip, Reply, Sparkles, Trash2 } from "lucide-react";
import type { ToolOutcome } from "../../lib/ipc";
import { useToolAction } from "../../lib/toolAction";
import { Composer, replySubject, type Draft } from "./Composer";
import { mailIpc, type MailBody, type MailRow } from "./api";

/** CSP du lecteur : rien n'est chargé sauf, sur demande pour ce mail seulement, les images https. */
export const csp = (images: boolean) => `default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:${images ? " https:" : ""}`;

const doc = (html: string, images: boolean) =>
  `<!doctype html><meta http-equiv="Content-Security-Policy" content="${csp(images)}"><style>body{margin:10px;font:14px/1.45 system-ui,sans-serif;color:#222;background:#fff;overflow-wrap:anywhere}img{max-width:100%;height:auto}</style>${html}`;

const size = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} Mo` : `${Math.max(1, Math.round(n / 1e3))} Ko`);

/** Lecteur en volet. Utiliser `key={mail.id}` : « Afficher les images » repart de zéro à chaque mail. */
export function MailReader({ mail, draft, onDraft, onClose }: { mail: MailRow; draft: Draft | null; onDraft: (d: Draft | null) => void; onClose: () => void }) {
  const [body, setBody] = useState<MailBody | null>(null);
  const [failed, setFailed] = useState(false);
  const [images, setImages] = useState(false);
  const [replying, setReplying] = useState(draft !== null);
  const { run, error } = useToolAction("Action impossible, réessaie.");

  useEffect(() => {
    let live = true;
    mailIpc.read(mail.account_id, mail.id).then((b) => live && setBody(b), () => live && setFailed(true));
    return () => { live = false; };
  }, [mail.account_id, mail.id]);

  const startReply = () => {
    setReplying(true);
    if (!draft) onDraft({ to: mail.from_addr, subject: replySubject(mail.subject), body: "" });
  };
  const gone = async (call: Promise<ToolOutcome>) => { if (await run(call)) onClose(); };

  return (
    <div className="mail-reader">
      <div className="mail-rbar">
        <button type="button" className="mail-btn mail-ico" aria-label="Retour" title="Retour" onClick={onClose}><ArrowLeft size={14} /></button>
        <button type="button" className="mail-btn" onClick={startReply}><Reply size={13} /> Répondre</button>
        <span className="mail-rsp" />
        <button type="button" className="mail-btn mail-ico" aria-label="Répondre avec IRIS" disabled title="Bientôt (IRIS)"><Sparkles size={14} /></button>
        <button type="button" className="mail-btn mail-ico" aria-label="Archiver" title="Archiver" onClick={() => void gone(mailIpc.archive(mail.account_id, mail.id))}><Archive size={14} /></button>
        <button type="button" className="mail-btn mail-ico" aria-label="Supprimer" title="Supprimer" onClick={() => void gone(mailIpc.delete(mail.account_id, mail.id))}><Trash2 size={14} /></button>
        <button type="button" className="mail-btn mail-ico" aria-label="Ouvrir dans Gmail" title="Ouvrir dans Gmail" onClick={() => void run(mailIpc.openWeb(mail.account_id, mail.id))}><ExternalLink size={14} /></button>
      </div>
      <h4 className="mail-rsubj">{mail.subject}</h4>
      <p className="mail-rfrom">{mail.from_name} &lt;{mail.from_addr}&gt;</p>
      {error && <p className="err" role="alert">{error}</p>}
      {failed && <p className="err" role="alert">Mail indisponible, réessaie.</p>}
      {body && (
        <>
          {!images && <button type="button" className="mail-btn" onClick={() => setImages(true)}><ImageIcon size={13} /> Afficher les images</button>}
          <div className="mail-frame-box"><iframe className="mail-frame" title="Message" sandbox="allow-popups-to-escape-sandbox" srcDoc={doc(body.html, images)} /></div>
          {body.attachments.length > 0 && (
            <ul className="mail-att">{body.attachments.map((a, i) => <li key={i}><Paperclip size={12} /> {a.filename} · {size(a.size)}</li>)}</ul>
          )}
          {body.links.length > 0 && (
            <div className="mail-links">
              <h5>Liens du message</h5>
              <ul>{body.links.map((u) => (
                <li key={u}><button type="button" title={u} onClick={() => void run(mailIpc.openLink(u))}>{u}</button></li>
              ))}</ul>
            </div>
          )}
        </>
      )}
      {replying && draft && <Composer mail={mail} draft={draft} onDraft={onDraft} onSent={() => { onDraft(null); setReplying(false); }} />}
    </div>
  );
}
