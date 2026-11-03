import { useEffect, useState } from "react";

const DAYS = ["Dimanche", "Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi"];
const MONTHS = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const pad = (n: number) => String(n).padStart(2, "0");

/** Position de `now` sur la frise [startHour, endHour], bornée entre 0 et 1. */
export function nowPosition(now: Date, startHour: number, endHour: number): number {
  const h = now.getHours() + now.getMinutes() / 60 + now.getSeconds() / 3600;
  return Math.min(1, Math.max(0, (h - startHour) / (endHour - startHour)));
}

export function greeting(now: Date, name: string): string {
  const g = now.getHours() < 18 ? "Bonjour" : "Bonsoir";
  return name.trim() ? `${g} ${name.trim()}` : g;
}

export const formatLongDate = (d: Date) => `${DAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]}`;
export const hhmm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
export const ss = (d: Date) => pad(d.getSeconds());

/**
 * Heure courante, rafraîchie à chaque frontière de `stepMs` (seconde, minute) par un seul
 * timeout réaligné — pas d'intervalle. Inactif (`active = false`) : aucun minuteur.
 */
export function useNow(stepMs: number, active = true): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    if (!active) return;
    let t: ReturnType<typeof setTimeout>;
    const tick = () => {
      const d = new Date();
      setNow(d);
      t = setTimeout(tick, stepMs - (d.getTime() % stepMs));
    };
    tick();
    return () => clearTimeout(t);
  }, [stepMs, active]);
  return now;
}
