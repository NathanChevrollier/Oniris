import { useState, type KeyboardEvent } from "react";
import { Cloud, CloudDrizzle, CloudFog, CloudLightning, CloudRain, CloudSnow, CloudSun, Sun, type LucideIcon } from "lucide-react";
import { useTopic } from "../../lib/data";
import { ipc } from "../../lib/ipc";
import { useSettings } from "../../lib/settings";
import { hhmm, useNow } from "../../lib/time";
import { usePanelStore } from "../../panel/store";
import { weatherIpc, type Weather } from "./api";
import "./weather.css";

const STALE_MS = 3 * 60 * 60 * 1000;
const ICONS: Record<string, LucideIcon> = { Sun, CloudSun, Cloud, CloudFog, CloudDrizzle, CloudRain, CloudSnow, CloudLightning };

/** « 13h05 ». */
const atTime = (ms: number) => hhmm(new Date(ms)).replace(":", "h");

/** Météo sous l'horloge : ville vide = lien « Choisir une ville » ; donnée de plus de 3 h = grisée avec l'heure de la mesure. */
export function WeatherBadge() {
  const open = usePanelStore((s) => s.open);
  const now = useNow(60_000, open);
  const settings = useSettings();
  const city = settings.weather_city;
  const { data } = useTopic<Weather | null>("weather", weatherIpc.get, null);
  const [editing, setEditing] = useState(false);

  const save = (value: string) => {
    setEditing(false);
    const weather_city = value.trim();
    if (weather_city !== city) ipc.saveSettings({ ...settings, weather_city }).catch((e) => console.warn("[oniris] ville", e));
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") save(e.currentTarget.value);
    else if (e.key === "Escape") { e.stopPropagation(); setEditing(false); } // ne ferme pas le panneau
  };

  if (editing) {
    return (
      <div className="w">
        <input className="wcity" autoFocus defaultValue={city} maxLength={80} placeholder="Ville" aria-label="Ville de la météo"
          spellCheck={false} onKeyDown={onKey} onBlur={() => setEditing(false)} />
      </div>
    );
  }
  if (!city) {
    return <div className="w"><button type="button" className="wlink" onClick={() => setEditing(true)}>Choisir une ville</button></div>;
  }
  if (!data) return <div className="w stale">Météo…</div>;

  const old = now.getTime() - data.fetched_ms > STALE_MS;
  const Icon = ICONS[data.icon] ?? Cloud;
  return (
    <div className={old ? "w stale" : "w"}>
      <Icon size={14} aria-hidden />
      <span>{Math.round(data.temp_c)}° · {data.rain_from_hour != null ? `pluie vers ${data.rain_from_hour} h` : data.label}</span>
      <button type="button" className="wplace" title="Changer de ville" onClick={() => setEditing(true)}>{data.place}</button>
      {old && <span className="wat">à {atTime(data.fetched_ms)}</span>}
    </div>
  );
}
