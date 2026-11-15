/**
 * Calculs, conversions d'unités et de devises tapés dans la barre. Parseur maison à descente récursive :
 * jamais d'eval ni de Function, seuls des nombres, + - * / ^ %, parenthèses et « de » après un pourcentage.
 */

type Tok = { t: "num"; v: number } | { t: "op"; v: string };

/** Découpe ; `null` dès qu'un caractère n'a rien à faire dans un calcul. */
function tokens(s: string): Tok[] | null {
  const out: Tok[] = [];
  const re = /\s*(?:(\d+(?:[.,]\d+)?|[.,]\d+)|(\*\*|[-+*/^%()×x÷:])|(de|of)\b)/iy;
  let i = 0;
  while (i < s.length) {
    if (/^\s+$/.test(s.slice(i))) break;
    re.lastIndex = i;
    const m = re.exec(s);
    if (!m) return null;
    i = re.lastIndex;
    if (m[1]) out.push({ t: "num", v: Number(m[1].replace(",", ".")) });
    else if (m[2]) out.push({ t: "op", v: ({ "**": "^", "×": "*", x: "*", X: "*", "÷": "/", ":": "/" } as Record<string, string>)[m[2]] ?? m[2] });
    else out.push({ t: "op", v: "of" });
  }
  return out;
}

/** Valeur d'une expression, `null` si elle est invalide ou non finie. */
export function evaluate(src: string): number | null {
  const tk = tokens(src);
  if (!tk || tk.length === 0) return null;
  let p = 0;
  const peek = () => tk[p];
  const isOp = (v: string) => peek()?.t === "op" && peek().v === v;
  const fail = (): never => { throw new Error("calc"); };

  // Un terme peut se terminer par un pourcentage nu : « 240 + 15 % » ajoute 15 % de 240.
  const expr = (): number => {
    let a = term().v;
    while (isOp("+") || isOp("-")) {
      const op = tk[p++].v;
      const b = term();
      const d = b.pct ? a * b.v : b.v;
      a = op === "+" ? a + d : a - d;
    }
    return a;
  };
  const term = (): { v: number; pct: boolean } => {
    let r = unary();
    while (isOp("*") || isOp("/")) {
      const op = tk[p++].v;
      const b = unary().v;
      r = { v: op === "*" ? r.v * b : r.v / b, pct: false };
    }
    return r;
  };
  const unary = (): { v: number; pct: boolean } => {
    if (isOp("-")) { p++; const r = unary(); return { v: -r.v, pct: r.pct }; }
    if (isOp("+")) { p++; return unary(); }
    return power();
  };
  const power = (): { v: number; pct: boolean } => {
    const base = postfix();
    if (!isOp("^")) return base;
    p++;
    return { v: base.v ** unary().v, pct: false };
  };
  const postfix = (): { v: number; pct: boolean } => {
    let v = primary();
    let pct = false;
    while (isOp("%")) { p++; v /= 100; pct = true; }
    if (pct && isOp("of")) { p++; return { v: v * unary().v, pct: false }; }
    return { v, pct };
  };
  const primary = (): number => {
    const x = tk[p++] ?? fail();
    if (x.t === "num") return x.v;
    if (x.v === "(") { const v = expr(); if (!isOp(")")) fail(); p++; return v; }
    return fail();
  };

  try {
    const v = expr();
    return p === tk.length && Number.isFinite(v) ? v : null;
  } catch { return null; }
}

const fmt = (n: number, digits: number, grouping = true) =>
  new Intl.NumberFormat("fr-FR", { maximumSignificantDigits: digits, useGrouping: grouping }).format(n);

/** Résultat prêt à afficher (`title`) et à copier (`copy`, sans séparateur de milliers). */
export type CalcResult = { title: string; subtitle: string; copy: string };

/** « (12+8)*3 » ou « 15% de 240 » ; rien pour un nombre seul ou une date « 2026-11-14 ». */
export function calc(text: string): CalcResult | null {
  const s = text.trim();
  if (!/\d/.test(s) || /^[-+]?[\d.,]+$/.test(s) || /^\d+(-\d+)+$/.test(s)) return null; // nombre seul, date ou référence
  const v = evaluate(s);
  if (v === null) return null;
  return { title: `= ${fmt(v, 12)}`, subtitle: `${s} · Entrée pour copier`, copy: fmt(v, 12, false) };
}

/* Unités : facteur vers l'unité de base de chaque grandeur (températures à part). */
const UNITS: Record<string, [dim: string, factor: number]> = {};
const def = (dim: string, factor: number, ...names: string[]) => { for (const n of names) UNITS[n] = [dim, factor]; };
def("len", 0.001, "mm"); def("len", 0.01, "cm"); def("len", 1, "m", "mètre", "mètres", "metre", "metres");
def("len", 1000, "km"); def("len", 0.0254, "in", "po", "pouce", "pouces"); def("len", 0.3048, "ft", "pied", "pieds");
def("len", 0.9144, "yd"); def("len", 1609.344, "mi", "mile", "miles");
def("mass", 0.001, "mg"); def("mass", 1, "g"); def("mass", 1000, "kg"); def("mass", 1e6, "t", "tonne", "tonnes");
def("mass", 453.59237, "lb", "lbs", "livre", "livres"); def("mass", 28.349523125, "oz");
def("vol", 0.001, "ml"); def("vol", 0.01, "cl"); def("vol", 1, "l", "litre", "litres"); def("vol", 3.785411784, "gal");
// Octets en puissances de 1024, comme l'Explorateur de Windows.
def("data", 1, "o", "b", "octet", "octets"); def("data", 1024, "ko", "kb"); def("data", 1024 ** 2, "mo", "mb");
def("data", 1024 ** 3, "go", "gb"); def("data", 1024 ** 4, "to", "tb");
def("time", 0.001, "ms"); def("time", 1, "s", "sec", "seconde", "secondes"); def("time", 60, "min", "minute", "minutes");
def("time", 3600, "h", "heure", "heures"); def("time", 86400, "j", "jour", "jours", "d"); def("time", 604800, "sem", "semaine", "semaines");
def("speed", 1 / 3.6, "km/h", "kmh"); def("speed", 1, "m/s"); def("speed", 0.44704, "mph");
const TEMPS: Record<string, "c" | "f" | "k"> = { "°c": "c", c: "c", "°f": "f", f: "f", k: "k" };

const toC = (v: number, u: "c" | "f" | "k") => (u === "c" ? v : u === "f" ? (v - 32) * 5 / 9 : v - 273.15);
const fromC = (v: number, u: "c" | "f" | "k") => (u === "c" ? v : u === "f" ? v * 9 / 5 + 32 : v + 273.15);

/** Une quantité « 5 km en mi », « 100 °F to °C » : nombre (ou calcul), unité, mot de liaison, unité. */
const CONV = /^(.+?)\s*([a-zA-Z°/µéè]+)\s+(?:en|in|to|vers|=|->)\s+([a-zA-Z°/µéè]+)$/i;

export function convertUnits(text: string): CalcResult | null {
  const m = CONV.exec(text.trim());
  if (!m) return null;
  const v = evaluate(m[1]);
  const [a, b] = [m[2].toLowerCase(), m[3].toLowerCase()];
  if (v === null) return null;
  let out: number;
  if (TEMPS[a] && TEMPS[b]) out = fromC(toC(v, TEMPS[a]), TEMPS[b]);
  else if (UNITS[a] && UNITS[b] && UNITS[a][0] === UNITS[b][0]) out = (v * UNITS[a][1]) / UNITS[b][1];
  else return null;
  return { title: `= ${fmt(out, 6)} ${m[3]}`, subtitle: `${fmt(v, 12)} ${m[2]} en ${m[3]} · Entrée pour copier`, copy: fmt(out, 6, false) };
}

/** Devises de la BCE (Frankfurter) ; symboles courants en plus des codes. */
const CODES = "AUD BGN BRL CAD CHF CNY CZK DKK EUR GBP HKD HUF IDR ILS INR ISK JPY KRW MXN MYR NOK NZD PHP PLN RON SEK SGD THB TRY USD ZAR".split(" ");
const SYMBOLS: Record<string, string> = { "€": "EUR", euro: "EUR", euros: "EUR", $: "USD", dollar: "USD", dollars: "USD", "£": "GBP", "¥": "JPY", yen: "JPY" };
const code = (s: string) => SYMBOLS[s.toLowerCase()] ?? (CODES.includes(s.toUpperCase()) ? s.toUpperCase() : null);
const MONEY = /^(?:([€$£¥]|[a-z]{3})\s*)?(\d+(?:[.,]\d+)?)\s*([€$£¥]|[a-zé]+)?\s+(?:en|in|to|vers|=|->)\s+([€$£¥]|[a-zé]+)$/i;

export type MoneyQuery = { amount: number; from: string; to: string };

/** « 30 € en $ », « $12 en EUR », « 100 chf to eur » ; `null` si ce n'est pas une conversion de devises connue. */
export function parseMoney(text: string): MoneyQuery | null {
  const m = MONEY.exec(text.trim());
  if (!m || (m[1] ? m[3] : !m[3])) return null; // devise d'origine avant OU après le montant
  const from = code(m[1] ?? m[3]);
  const to = code(m[4]);
  if (!from || !to || from === to) return null;
  return { amount: Number(m[2].replace(",", ".")), from, to };
}

/** Conversion avec des taux « pour 1 EUR » ; `null` si une devise manque. */
export function convertMoney(q: MoneyQuery, rates: Record<string, number>): CalcResult | null {
  const [a, b] = [rates[q.from], rates[q.to]];
  if (!a || !b) return null;
  const out = (q.amount * b) / a;
  const money = (n: number, c: string, grouping = true) =>
    new Intl.NumberFormat("fr-FR", { style: "currency", currency: c, useGrouping: grouping }).format(n);
  return {
    title: `= ${money(out, q.to)}`,
    subtitle: `${money(q.amount, q.from)} en ${q.to} · taux BCE · Entrée pour copier`,
    copy: new Intl.NumberFormat("fr-FR", { minimumFractionDigits: 2, maximumFractionDigits: 2, useGrouping: false }).format(out),
  };
}
