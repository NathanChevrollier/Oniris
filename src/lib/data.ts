import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "./ipc";
import { usePanelStore } from "../panel/store";

export type Topic = "agenda" | "reminders" | "notes" | "timers" | "clipboard" | "weather";

/** Données d'un sujet : chargées à la première ouverture, rechargées sur `oniris://changed` (panneau ouvert) ou à la réouverture si périmées. */
export function useTopic<T>(topic: Topic, fetcher: () => Promise<T>, initial: T): { data: T; reload: () => void } {
  const open = usePanelStore((s) => s.open);
  const [data, setData] = useState<T>(initial);
  const f = useRef(fetcher);
  f.current = fetcher;
  const loaded = useRef(false);
  const stale = useRef(false);
  const openRef = useRef(open);
  openRef.current = open;

  const reload = useCallback(() => {
    loaded.current = true;
    stale.current = false;
    f.current().then(setData, (e) => console.warn("[oniris] chargement", topic, e));
  }, [topic]);

  useEffect(() => {
    if (open && (!loaded.current || stale.current)) reload();
  }, [open, reload]);

  useEffect(() => {
    const un = listen<{ topic: Topic }>("oniris://changed", (e) => {
      if (e.payload.topic !== topic) return;
      if (openRef.current) reload();
      else stale.current = true;
    });
    return () => { un.then((u) => u()); };
  }, [topic, reload]);

  return { data, reload };
}
