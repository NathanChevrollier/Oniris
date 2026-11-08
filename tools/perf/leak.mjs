// Fonctions pures du test de fuite (testées par soak.test.mjs).
// samples : [{ t (ms), mb }]. Pente par moindres carrés, en Mo/h.
export function slope(samples) {
  const n = samples.length;
  if (n < 2) return null;
  const mt = samples.reduce((s, p) => s + p.t, 0) / n;
  const mm = samples.reduce((s, p) => s + p.mb, 0) / n;
  let num = 0, den = 0;
  for (const p of samples) { num += (p.t - mt) * (p.mb - mm); den += (p.t - mt) ** 2; }
  return den === 0 ? null : (num / den) * 3_600_000;
}

// Exclut la fenêtre de chauffe puis juge la pente.
// t = ms écoulées depuis le début du run. Moins de minSamples points après chauffe : pas de verdict.
export function verdict(samples, { skipMs = 3_600_000, thresholdMbPerH = 1, minSamples = 12 } = {}) {
  const kept = samples.filter((p) => p.t >= skipMs);
  const s = kept.length < minSamples ? null : slope(kept);
  return { slopeMbPerH: s, used: kept.length, leak: s !== null && s > thresholdMbPerH, insufficient: s === null };
}
