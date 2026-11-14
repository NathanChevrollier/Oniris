import { applyGlass } from "../../panel/surface";
import { Group, Row, Segmented, Slider, useSet } from "./controls";

/** Section « Apparence » (catégorie Général) : verre du panneau et hauteur ; le panneau et cette feuille servent d'aperçu. */
export function Appearance() {
  const { s, save } = useSet();
  return (
    <Group title="Apparence">
      <Segmented k="glass_mode" label="Verre" options={[["auto", "Automatique"], ["manual", "Manuel"]]}
        desc="Automatique : plus opaque sur un fond d'écran clair ou chargé, plus transparent sur un fond sombre et uni. Mesuré à chaque ouverture, rien n'est enregistré." />
      {s.glass_mode === "manual" && (
        <Slider k="glass_opacity" label="Intensité du verre" min={20} max={95} unit=" %" onLive={(v) => applyGlass(v / 100)}
          desc="Opacité du fond ; le flou suit." />
      )}
      <Row label="Hauteur du panneau" desc={s.panel_height_pct ? `${s.panel_height_pct} % de l'écran. Glisse la poignée en bas du panneau, double-clic pour revenir à l'automatique.` : "Automatique. Glisse la poignée en bas du panneau pour la changer."}>
        {s.panel_height_pct > 0 && <button type="button" className="acc-btn" onClick={() => void save({ panel_height_pct: 0 })}>Automatique</button>}
      </Row>
    </Group>
  );
}
