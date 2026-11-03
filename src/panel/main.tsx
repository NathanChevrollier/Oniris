import { createRoot } from "react-dom/client";
import { listen, isTauri } from "../lib/ipc";
import { Panel } from "./Panel";

if (!isTauri) document.documentElement.classList.add("preview");
else installPerfProbe();
createRoot(document.getElementById("root")!).render(<Panel />);

/** Banc de mesure : armé par `oniris://perf-armed` (émis par perf_open_cycles), sinon aucun IPC. */
function installPerfProbe() {
  let armed = false;
  listen<boolean>("oniris://perf-armed", (e) => { armed = e.payload; });
  listen<{ open: boolean }>("oniris://panel-state", (e) => {
    if (!armed || !e.payload.open) return;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      import("@tauri-apps/api/core").then((m) => m.invoke("perf_frame", { atMs: Date.now() }));
    }));
  });
}
