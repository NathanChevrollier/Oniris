import { useCallback, useEffect, useState } from "react";
import { errorText } from "../accounts/api";
import { projectsIpc, type Project } from "./api";

/** Projets masqués (réglages) : chacun peut être ré-affiché. */
export function HiddenProjects() {
  const [list, setList] = useState<Project[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(() => { projectsIpc.listHidden().then(setList, () => setErr("Impossible de charger les projets masqués.")); }, []);
  useEffect(load, [load]);
  return (
    <>
      <h4>Projets masqués</h4>
      {err && <p className="err" role="alert">{err}</p>}
      {list.length === 0 && !err ? <p className="acc-empty">Aucun projet masqué.</p> : (
        <ul className="acc-list">{list.map((p) => (
          <li key={p.path} className="acc-row"><div className="acc-main"><span className="acc-label" title={p.path}>{p.name}</span>
            <button type="button" className="acc-btn" aria-label={`Ré-afficher ${p.name}`}
              onClick={() => void projectsIpc.hide(p.path, false).then(() => { setErr(null); load(); }, (e) => setErr(errorText(e, "Ré-affichage impossible.")))}>Ré-afficher</button></div></li>
        ))}</ul>
      )}
    </>
  );
}
