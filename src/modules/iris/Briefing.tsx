import { useState } from "react";
import { Sparkles } from "lucide-react";
import { useTopic } from "../../lib/data";
import { irisBriefingIpc, type BriefingData } from "./api";
import "./iris.css";

/** Texte d'un tiers : italique, filet à gauche, source en infobulle (partagé avec la conversation). */
export function SegmentText({ s }: { s: BriefingData["segments"][number] }) {
  return s.t === "Text" ? <>{s.text}</> : <q className="iris-untrusted" title={`Source : ${s.source}`}>{s.text}</q>;
}

/** Briefing de l'en-tête : le dernier connu s'affiche tout de suite, le nouveau le remplace quand il arrive. */
export function Briefing() {
  const { data } = useTopic<BriefingData | null>("iris", irisBriefingIpc.get, null);
  const [error, setError] = useState(false);
  const act = (i: number) => {
    setError(false);
    irisBriefingIpc.act(i).catch(() => setError(true));
  };
  return (
    <div className="brief">
      <div className="spark"><Sparkles size={15} /></div>
      <div>
        <p>{data ? data.segments.map((s, i) => <SegmentText key={i} s={s} />) : "IRIS prépare ton briefing…"}</p>
        {data && data.suggestions.length > 0 && (
          <div className="iris-chips">
            {data.suggestions.map((s, i) => <button key={i} type="button" className="iris-chip" onClick={() => act(i)}>{s.label}</button>)}
          </div>
        )}
        {error && <p className="err" role="alert">Action impossible, réessaie.</p>}
      </div>
    </div>
  );
}
