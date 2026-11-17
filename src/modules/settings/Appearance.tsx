import { applyGlass, glassAlpha, useGlassMeasure, type GlassMeasure } from "../../panel/surface";
import { Group, Row, Segmented, Slider, Toggle, useSet } from "./controls";

const TONE: Record<GlassMeasure["tone"], string> = {
  sombre: "fond sombre → verre léger",
  "chargé": "fond chargé → verre renforcé",
  clair: "fond clair → verre plus opaque",
};

/** Section « Apparence » (catégorie Général) : verre du panneau et hauteur ; le panneau et cette feuille servent d'aperçu. */
export function Appearance() {
  const { s, save } = useSet();
  const m = useGlassMeasure();
  const auto = s.glass_mode === "auto";
  const app = s.glass_app_boost && !!m && m.app > 0;
  const live = m && `Auto : ${app ? "application derrière → verre plus opaque et plus flou" : TONE[m.tone]} (${Math.round(glassAlpha("auto", s.glass_opacity, m, s.glass_app_boost) * 100)} %).`;
  return (
    <Group title="Apparence">
      <Segmented k="glass_mode" label="Verre" options={[["auto", "Automatique"], ["manual", "Manuel"]]}
        desc="Automatique : plus opaque sur un fond clair ou chargé, ou devant une application ; plus transparent sur un fond sombre et uni. Mesuré panneau ouvert, rien n'est enregistré." />
      {auto && <Row label="Mesure en direct" desc={<span aria-live="polite">{live ?? "Auto : mesure à la prochaine ouverture du panneau."}</span>} />}
      {auto && <Toggle k="glass_app_boost" label="Plus opaque quand une application est derrière" desc="Sur le bureau seul, le verre reste léger et laisse voir le fond d'écran." />}
      <Slider k="glass_opacity" label={auto ? "Préférence de verre" : "Intensité du verre"} min={20} max={95} unit=" %"
        onLive={(v) => applyGlass(glassAlpha(s.glass_mode, v, m, s.glass_app_boost))}
        desc={auto ? "Point de départ de l'automatique : plus haut, plus opaque que la mesure." : "Opacité du fond ; le flou suit."} />
      <Row label="Hauteur du panneau" desc={s.panel_height_pct ? `${s.panel_height_pct} % de l'écran. Glisse la poignée en bas du panneau, double-clic pour revenir à l'automatique.` : "Automatique. Glisse la poignée en bas du panneau pour la changer."}>
        {s.panel_height_pct > 0 && <button type="button" className="acc-btn" onClick={() => void save({ panel_height_pct: 0 })}>Automatique</button>}
      </Row>
    </Group>
  );
}
