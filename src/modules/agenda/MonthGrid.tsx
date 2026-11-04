const LETTERS: [string, string][] = [["L", "lundi"], ["M", "mardi"], ["M", "mercredi"], ["J", "jeudi"], ["V", "vendredi"], ["S", "samedi"], ["D", "dimanche"]];
const MONTHS = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/** Grille du mois, semaine du lundi au dimanche. `month` : 1 à 12 ; `today`/`selected` : numéro de jour ou null. */
export function MonthGrid({ year, month, today, selected, withEvents, onSelect }: {
  year: number; month: number; today: number | null; selected: number | null; withEvents: number[]; onSelect: (day: number) => void;
}) {
  const lead = (new Date(year, month - 1, 1).getDay() + 6) % 7;
  const count = new Date(year, month, 0).getDate();
  return (
    <div className="cal">
      {LETTERS.map(([l, full], i) => <div key={i} className="d"><abbr title={full}>{l}</abbr></div>)}
      {Array.from({ length: lead }, (_, i) => <span key={`b${i}`} aria-hidden />)}
      {Array.from({ length: count }, (_, i) => {
        const day = i + 1;
        const wd = (lead + i) % 7;
        const has = withEvents.includes(day);
        const cls = [day === today && "today", day === selected && "sel", has && "has", wd >= 5 && "we"].filter(Boolean).join(" ");
        return (
          <button key={day} type="button" className={cls} aria-pressed={day === selected} aria-current={day === today ? "date" : undefined}
            aria-label={`${day} ${MONTHS[month - 1]} ${year}${has ? ", avec des événements" : ""}`} onClick={() => onSelect(day)}>{day}</button>
        );
      })}
    </div>
  );
}
