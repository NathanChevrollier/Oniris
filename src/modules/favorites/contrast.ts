/** Dégradé des initiales : saturation et luminosités fixes, choisies pour que le texte blanc reste lisible sur toutes les teintes. */
export const SAT = 55;
export const LIGHT = 30;
export const DARK = 22;
export const hsl = (h: number, l: number) => `hsl(${h} ${SAT}% ${l}%)`;

/** HSL (h en degrés, s et l en %) vers RGB 0-255. */
export function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const S = s / 100, L = l / 100, a = S * Math.min(L, 1 - L);
  const f = (n: number) => { const k = (n + h / 30) % 12; return 255 * (L - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))); };
  return [f(0), f(8), f(4)];
}

const lin = (c: number) => { const v = c / 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
const luminance = ([r, g, b]: [number, number, number]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);

/** Rapport de contraste WCAG entre deux couleurs RGB 0-255. */
export function contrastRatio(fg: [number, number, number], bg: [number, number, number]) {
  const [a, b] = [luminance(fg), luminance(bg)].sort((x, y) => y - x);
  return (a + 0.05) / (b + 0.05);
}
