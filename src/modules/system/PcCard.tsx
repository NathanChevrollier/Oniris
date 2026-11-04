import { useEffect, useState } from "react";
import { Card } from "../../ui/Card";
import { isTauri } from "../../lib/ipc";
import { usePanelStore } from "../../panel/store";
import { FAKE_STATUS, systemIpc, type SystemStatus } from "./api";
import { Gauge } from "./Gauge";
import "./system.css";

const pct = (used: number, total: number) => (total > 0 ? (used / total) * 100 : 0);

export function PcCard() {
  const open = usePanelStore((s) => s.open);
  const [st, setSt] = useState<SystemStatus | null>(isTauri ? null : FAKE_STATUS);

  // Abonné seulement panneau ouvert : fermé, plus d'écoute (et l'échantillonneur Rust est arrêté).
  useEffect(() => {
    if (!open) return;
    let live = true;
    const off = systemIpc.subscribe((s) => { if (live) setSt(s); });
    return () => { live = false; off(); };
  }, [open]);

  const vram = st && st.vram_used_mb != null && st.vram_total_mb ? pct(st.vram_used_mb, st.vram_total_mb) : null;
  return (
    <Card title="Ce PC" span={3} delay={0.25}>
      <div className="rings">
        <Gauge label="CPU" value={st?.cpu_pct ?? null} color="#30d158" />
        {(!st || st.gpu_pct != null) && <Gauge label="GPU" value={st?.gpu_pct ?? null} color="#64d2ff" />}
        <Gauge label="RAM" value={st ? pct(st.ram_used_mb, st.ram_total_mb) : null} color="#ffd60a" />
        {(!st || vram != null) && <Gauge label="VRAM" value={vram} color="#bf5af2" />}
      </div>
      <div className="ai-state">
        <div>IA locale <i>{st?.local_ai ?? "–"}</i></div>
        <div>Voix (Whisper) <i>{st?.voice ?? "–"}</i></div>
        <div>Assistant <i className="on">{st ? `● ${st.oniris_mb} Mo` : "–"}</i></div>
      </div>
    </Card>
  );
}
