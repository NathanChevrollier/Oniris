import { createRoot } from "react-dom/client";
import { onConfirm, toolIpc } from "../lib/ipc";
import { ConfirmQueue } from "./ConfirmQueue";

createRoot(document.getElementById("root")!).render(
  <>
    <div>panel</div>
    <ConfirmQueue subscribe={onConfirm} resolve={toolIpc.resolve} />
  </>,
);
