import { createRoot } from "react-dom/client";
import { isTauri } from "../lib/ipc";
import { Panel } from "./Panel";

if (!isTauri) document.documentElement.classList.add("preview");
createRoot(document.getElementById("root")!).render(<Panel />);
