import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "./ipc";
import { usePanelStore } from "../panel/store";

export type Topic = "agenda" | "reminders" | "notes" | "timers" | "clipboard" | "weather" | "favorites" | "projects";

/** Données d'un sujet : chargées à la première ouverture, rechargées sur `oniris://changed` (panneau ouvert) ou à la réouverture si périmées. */
export function useTopic<T>(topic: Topic, fetcher: () => Promise<T>, initial: T, key?: string): { data: T; reload: () => void } {
  const open = usePanelStore((s) => s.open);
  const [data, setData] = useState<T>(initial);
  const f = useRef(fetcher);
  f.current = fetcher;
  const loaded = useRef(false);
  const stale = useRef(false);
  const openRef = useRef(open);
  openRef.current = open;

  const req = useRef(0);
  useEffect(() => () => { req.current = -1; }, []); // après démontage, plus aucun résultat appliqué

  const reload = useCallback(() => {
    loaded.current = true;
    stale.current = false;
    const id = ++req.current;
    f.current().then(
      (v) => { if (req.current === id) setData(v); },
      (e) => {
        if (req.current === id) stale.current = true; // la prochaine ouverture réessaie
        console.warn("[oniris] chargement", topic, e);
      },
    );
  }, [topic]);

  useEffect(() => {
    if (open && (!loaded.current || stale.current)) reload();
  }, [open, reload]);

  // `key` (ex. le jour affiché) change : recharge si le panneau est ouvert, sinon marque périmé.
  const prevKey = useRef(key);
  useEffect(() => {
    if (prevKey.current === key) return;
    prevKey.current = key;
    if (openRef.current) reload();
    else stale.current = true;
  }, [key, reload]);

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
