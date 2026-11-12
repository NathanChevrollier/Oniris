//! Décision de mise à jour : pure, sans réseau ni horloge propre (l'appelant donne les instants).
//! `Instant` est celui de tokio, pour que l'appelant puisse piloter le temps pausé des tests.
use std::time::Duration;
use tokio::time::Instant;

pub const FIRST_CHECK: Duration = Duration::from_secs(180);
pub const INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// Vrai seulement si `remote` est strictement plus récente que `current` ; faux si l'une est illisible.
pub fn is_newer(current: &str, remote: &str) -> bool {
    match (semver::Version::parse(current), semver::Version::parse(remote)) {
        (Ok(c), Ok(r)) => r > c,
        _ => false,
    }
}

/// Prochain essai de vérification : 3 min après le démarrage, puis toutes les 6 h (échec compris).
pub struct Schedule {
    next: Instant,
}

impl Schedule {
    pub fn new(start: Instant) -> Self {
        Self { next: start + FIRST_CHECK }
    }
    pub fn due(&self, now: Instant, enabled: bool) -> bool {
        enabled && now >= self.next
    }
    pub fn done(&mut self, now: Instant) {
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
    fn first_check_after_three_minutes() {
        let t = Instant::now();
        let s = Schedule::new(t);
        assert!(!s.due(t + Duration::from_secs(179), true));
        assert!(s.due(t + Duration::from_secs(180), true));
    }

    #[test]
    fn failure_waits_full_interval() {
        let t = Instant::now();
        let mut s = Schedule::new(t);
        s.done(t);
        assert!(!s.due(t + Duration::from_secs(5 * 3600 + 59 * 60), true));
        assert!(s.due(t + Duration::from_secs(6 * 3600), true));
    }

    #[test]
    fn disabled_never_due() {
        let t = Instant::now();
        let s = Schedule::new(t);
        assert!(!s.due(t + Duration::from_secs(100 * 3600), false));
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
