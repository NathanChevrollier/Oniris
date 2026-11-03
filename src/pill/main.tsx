import { createRoot } from "react-dom/client";
import { isTauri } from "../lib/ipc";
import { Pill } from "./Pill";

if (!isTauri) document.documentElement.classList.add("preview");
createRoot(document.getElementById("root")!).render(<Pill />);
