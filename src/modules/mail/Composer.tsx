import { Send } from "lucide-react";
import { useToolAction } from "../../lib/toolAction";
import { mailIpc, type MailRow } from "./api";

export type Draft = { to: string; subject: string; body: string };

/** Sujet d'une réponse : « Re: » une seule fois. */
export const replySubject = (s: string) => (/^re\s*:/i.test(s) ? s : `Re: ${s}`);

/** Brouillon gardé par le parent (en mémoire, tant que le panneau est ouvert). « Envoyer » ouvre la carte de confirmation. */
export function Composer({ mail, draft, onDraft, onSent }: { mail: MailRow; draft: Draft; onDraft: (d: Draft) => void; onSent: () => void }) {
  const { run, error, pending } = useToolAction("Envoi impossible, réessaie.");
  const set = (k: keyof Draft) => (v: string) => onDraft({ ...draft, [k]: v });
  const send = async () => {
    if (await run(mailIpc.send(mail.account_id, draft.to, draft.subject, draft.body, mail.id))) onSent();
  };
  return (
    <form className="mail-compose" onSubmit={(e) => { e.preventDefault(); void send(); }}>
      <label>À<input value={draft.to} onChange={(e) => set("to")(e.target.value)} /></label>
      <label>Sujet<input value={draft.subject} onChange={(e) => set("subject")(e.target.value)} /></label>
      <label className="sr-only" htmlFor="mail-body">Message</label>
      <textarea id="mail-body" placeholder="Ta réponse…" value={draft.body} onChange={(e) => set("body")(e.target.value)} />
      {error && <p className="err" role="alert">{error}</p>}
      <button type="submit" className="mail-btn" disabled={pending || !draft.to.trim() || !draft.body.trim()}><Send size={13} /> Envoyer</button>
    </form>
  );
}
