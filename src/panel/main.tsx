import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { onConfirm, toolIpc, type PendingAction } from "../lib/ipc";
import { ConfirmCard } from "./ConfirmCard";

function Panel() {
  const [pending, setPending] = useState<PendingAction | null>(null);
  useEffect(() => {
    const un = onConfirm(setPending);
    return () => { un.then((f) => f()); };
  }, []);
  return pending ? (
    <ConfirmCard pending={pending} onResolve={(ok) => { toolIpc.resolve(pending.action_id, ok); setPending(null); }} />
  ) : <div>panel</div>;
}

createRoot(document.getElementById("root")!).render(<Panel />);
