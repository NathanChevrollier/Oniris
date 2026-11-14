//! Décision de mise à jour : pure, sans réseau ni horloge propre (l'appelant donne les instants).
//! `Instant` est celui de tokio, pour que l'appelant puisse piloter le temps pausé des tests.
use std::time::Duration;
use tokio::time::Instant;

pub const FIRST_CHECK: Duration = Duration::from_secs(45);
pub const INTERVAL: Duration = Duration::from_secs(2 * 3600);
/// Ouverture du panneau : vérifie si la dernière vérification est plus vieille que ça.
pub const STALE: Duration = Duration::from_secs(3600);
/// Jamais deux vérifications automatiques plus rapprochées que ça.
pub const MIN_GAP: Duration = Duration::from_secs(10 * 60);
/// Sortie de veille : on laisse le réseau revenir avant de vérifier.
pub const WAKE_DELAY: Duration = Duration::from_secs(30);
/// Retard de l'horloge murale sur le pas de la boucle au-delà duquel le PC sortait de veille.
pub const WAKE_JUMP: Duration = Duration::from_secs(120);

/// Vrai seulement si `remote` est strictement plus récente que `current` ; faux si l'une est illisible.
pub fn is_newer(current: &str, remote: &str) -> bool {
    match (semver::Version::parse(current), semver::Version::parse(remote)) {
        (Ok(c), Ok(r)) => r > c,
        _ => false,
    }
}

/// L'horloge murale a avancé bien plus que le pas attendu : veille (ou changement d'heure, sans gravité).
pub fn slept(wall_elapsed: Duration, tick: Duration) -> bool {
    wall_elapsed >= tick + WAKE_JUMP
}

/// Calendrier des vérifications automatiques : 45 s après le démarrage, puis toutes les 2 h (échec compris),
/// plus tôt au réveil ou à l'ouverture du panneau, jamais à moins de `MIN_GAP` de la précédente.
pub struct Schedule {
    next: Instant,
    last: Option<Instant>,
}

impl Schedule {
    pub fn new(start: Instant) -> Self {
        Self { next: start + FIRST_CHECK, last: None }
    }
    pub fn due(&self, now: Instant, enabled: bool) -> bool {
        enabled && now >= self.next
    }
    /// Ouverture du panneau : vrai si aucune vérification depuis `STALE`.
    pub fn stale(&self, now: Instant, enabled: bool) -> bool {
        enabled && self.last.is_none_or(|l| now >= l + STALE)
    }
    /// Réveil de veille : avance la prochaine vérification à `WAKE_DELAY`, sans passer sous `MIN_GAP`.
    pub fn woke(&mut self, now: Instant) {
        let earliest = self.last.map_or(now, |l| l + MIN_GAP).max(now + WAKE_DELAY);
        self.next = self.next.min(earliest);
    }
    pub fn done(&mut self, now: Instant) {
        self.last = Some(now);
        self.next = now + INTERVAL;
    }
}

/// Ce qui empêche d'installer : pipeline voix occupé, plein écran.
#[derive(Clone, Copy)]
pub struct Calm {
    pub voice_busy: bool,
    pub fullscreen: bool,
}

pub fn can_install(c: Calm) -> bool {
    !c.voice_busy && !c.fullscreen
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn only_strictly_newer_versions() {
        assert!(is_newer("1.2.0", "1.2.1"));
        assert!(!is_newer("1.2.0", "1.2.0"));
        assert!(!is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("1.9.0", "1.10.0"));
        assert!(!is_newer("1.2.0", "pas-une-version"));
        assert!(!is_newer("pas-une-version", "1.2.0"));
    }

    #[test]
    fn first_check_shortly_after_startup() {
        let t = Instant::now();
        let s = Schedule::new(t);
        assert!(!s.due(t + S(44), true));
        assert!(s.due(t + S(45), true));
    }

    #[test]
    fn failure_waits_full_interval() {
        let t = Instant::now();
        let mut s = Schedule::new(t);
        s.done(t);
        assert!(!s.due(t + S(2 * 3600 - 1), true));
        assert!(s.due(t + S(2 * 3600), true));
    }

    #[test]
    fn disabled_never_due() {
        let t = Instant::now();
        let mut s = Schedule::new(t);
        s.woke(t);
        assert!(!s.due(t + S(100 * 3600), false));
        assert!(!s.stale(t + S(100 * 3600), false));
    }

    #[test]
    fn panel_checks_only_when_last_check_is_old() {
        let t = Instant::now();
        let mut s = Schedule::new(t);
        assert!(s.stale(t, true), "aucune vérification encore");
        s.done(t);
        assert!(!s.stale(t + S(3599), true));
        assert!(s.stale(t + S(3600), true));
    }

    #[test]
    fn wake_checks_soon_but_never_in_bursts() {
        let t = Instant::now();
        let mut s = Schedule::new(t);
        s.done(t);
        // Réveil longtemps après : vérification 30 s plus tard.
        let w = t + S(3000);
        s.woke(w);
        assert!(!s.due(w + S(29), true));
        assert!(s.due(w + S(30), true));
        // Réveil juste après une vérification : attend l'intervalle minimal.
        s.done(w + S(30));
        let w2 = w + S(90);
        s.woke(w2);
        assert!(!s.due(w + S(30) + MIN_GAP - S(1), true));
        assert!(s.due(w + S(30) + MIN_GAP, true));
        // Réveil avant la toute première vérification : avancée aussi.
        let mut f = Schedule::new(t);
        f.woke(t);
        assert!(f.due(t + S(30), true));
    }

    #[test]
    fn wall_clock_jump_means_sleep() {
        assert!(!slept(S(60), S(60)));
        assert!(!slept(S(179), S(60)));
        assert!(slept(S(180), S(60)));
        assert!(slept(S(8 * 3600), S(60)));
    }

    #[test]
    fn install_only_when_calm() {
        let c = |voice_busy, fullscreen| can_install(Calm { voice_busy, fullscreen });
        assert!(c(false, false));
        assert!(!c(true, false));
        assert!(!c(false, true));
        assert!(!c(true, true));
    }
}
