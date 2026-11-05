import { useEffect, useMemo, useState } from "react";
import { FolderGit2 } from "lucide-react";
import { Card } from "../../ui/Card";
import { Empty } from "../../ui/Empty";
import { useTopic } from "../../lib/data";
import { useNow } from "../../lib/time";
import { useToolAction } from "../../lib/toolAction";
import { focusPalette } from "../../lib/focusCard";
import type { ToolOutcome } from "../../lib/ipc";
import { usePanelStore } from "../../panel/store";
import { projectsIpc, type Project, type ResumeMode } from "./api";
import { ProjectTile } from "./ProjectTile";
import "./projects.css";

const NONE: Project[] = [];
const SHOWN = 3;
const done = (): ToolOutcome => ({ Done: { action_id: 0, output: {} } });
const failed = (e: unknown): ToolOutcome => ({ Failed: { action_id: 0, error: String(e) } });

export function ProjectsCard() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open).getTime();
  // `null` : pas encore chargé (ni liste ni réglages) — rien n'est affiché, pour ne pas faire clignoter le sélecteur de dossier.
  const topic = useTopic<Project[] | null>("projects", projectsIpc.list, null);
  const data = topic.data ?? NONE;
  const [roots, setRootsState] = useState<string[] | null>(null);
  useEffect(() => projectsIpc.roots(setRootsState), []);
  const [changing, setChanging] = useState(false);
  const { run, error, pending, clear } = useToolAction("Action impossible pour le moment.");
  const [info, setInfo] = useState<string | null>(null);
  const [dir, setDir] = useState("");
  const [rootsError, setRootsError] = useState<string | null>(null);

  const shown = useMemo(() => [...data].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.last_activity_ms - a.last_activity_ms).slice(0, SHOWN), [data]);

  const act = (call: Promise<unknown>) => { clear(); setInfo(null); void run(call.then(done, failed)); };
  const resume = (path: string, mode: ResumeMode) => {
    clear(); setInfo(null);
    void run(projectsIpc.resume(path, mode).then((m) => { setInfo(m); return done(); }, failed));
  };
  const setRoots = (path: string) => {
    const p = path.trim();
    if (!p) return;
    setRootsError(null);
    projectsIpc.setRoots([p]).then(() => setChanging(false), (e) => setRootsError(String(e) || "Dossier refusé, essaie un autre chemin."));
  };
  const browse = async () => { const p = await projectsIpc.browse().catch(() => null); if (p) setRoots(p); };

  const loaded = topic.data !== null && roots !== null;
  const chooser = loaded && (roots.length === 0 || changing);

  return (
    <Card title="Projets" span={6} delay={0.2}
      aside={loaded && !chooser && data.length > 0 && <button type="button" className="proj-all" onClick={() => focusPalette("p ")}>Tous les projets ({data.length})</button>}>
      {!loaded ? null : chooser ? (
        <>
          <Empty icon={FolderGit2} tint="#30d158">Choisis le dossier où sont tes projets</Empty>
          <form className="proj-roots" onSubmit={(e) => { e.preventDefault(); setRoots(dir); }}>
            <input aria-label="Dossier de tes projets" placeholder={String.raw`C:\Users\toi\Projets`} value={dir} onChange={(e) => setDir(e.target.value)} spellCheck={false} />
            <button type="button" onClick={() => void browse()}>Parcourir</button>
          </form>
          {rootsError && <p className="err" role="alert">{rootsError}</p>}
        </>
      ) : data.length === 0 ? (
        <>
          <Empty icon={FolderGit2} tint="#30d158">Aucun projet trouvé</Empty>
          <button type="button" className="proj-change" onClick={() => setChanging(true)}>Changer de dossier</button>
        </>
      ) : (
        <div className="projs">
          {shown.map((p) => (
            <ProjectTile key={p.path} p={p} now={now} busy={pending}
              onResume={(m) => resume(p.path, m)} onPin={() => act(projectsIpc.pin(p.path, !p.pinned))}
              onHide={() => act(projectsIpc.hide(p.path, true))} onOpenDir={() => act(projectsIpc.openDir(p.path))} />
          ))}
        </div>
      )}
      <p className="proj-info" role="status">{info}</p>
      {error && <p className="err" role="alert">{error}</p>}
    </Card>
  );
}
