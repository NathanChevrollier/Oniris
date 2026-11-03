//! Distingue l'appui court (bascule du panneau) du maintien (push-to-talk).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    ChordDown { at_ms: u64 },
    ChordUp { at_ms: u64 },
    Tick { at_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    TogglePanel,
    PttStart,
    PttEnd,
}

#[derive(Clone, Copy)]
enum State {
    Idle,
    Held { since: u64, ptt_started: bool },
}

pub struct Classifier {
    threshold_ms: u64,
    state: State,
}

impl Classifier {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            threshold_ms,
            state: State::Idle,
        }
    }

    pub fn feed(&mut self, ev: KeyEvent) -> Option<HotkeyAction> {
        match (self.state, ev) {
            (State::Idle, KeyEvent::ChordDown { at_ms }) => {
                self.state = State::Held {
                    since: at_ms,
                    ptt_started: false,
                };
                None
            }
            (
                State::Held {
                    since,
                    ptt_started: false,
                },
                KeyEvent::Tick { at_ms },
            ) if at_ms.saturating_sub(since) >= self.threshold_ms => {
                self.state = State::Held {
                    since,
                    ptt_started: true,
                };
                Some(HotkeyAction::PttStart)
            }
            (State::Held { ptt_started, .. }, KeyEvent::ChordUp { .. }) => {
                self.state = State::Idle;
                Some(if ptt_started {
                    HotkeyAction::PttEnd
                } else {
                    HotkeyAction::TogglePanel
                })
            }
            // Répétition automatique, Tick hors maintien ou relâchement sans appui.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appui_court_bascule_au_relachement() {
        let mut c = Classifier::new(250);
        assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 0 }), None);
        assert_eq!(
            c.feed(KeyEvent::ChordUp { at_ms: 120 }),
            Some(HotkeyAction::TogglePanel)
        );
    }

    #[test]
    fn maintien_demarre_puis_termine_le_ptt() {
        let mut c = Classifier::new(250);
        c.feed(KeyEvent::ChordDown { at_ms: 0 });
        assert_eq!(c.feed(KeyEvent::Tick { at_ms: 249 }), None);
        assert_eq!(
            c.feed(KeyEvent::Tick { at_ms: 250 }),
            Some(HotkeyAction::PttStart)
        );
        assert_eq!(c.feed(KeyEvent::Tick { at_ms: 400 }), None);
        assert_eq!(
            c.feed(KeyEvent::ChordUp { at_ms: 900 }),
            Some(HotkeyAction::PttEnd)
        );
    }

    #[test]
    fn repetition_automatique_ignoree() {
        let mut c = Classifier::new(250);
        c.feed(KeyEvent::ChordDown { at_ms: 0 });
        assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 30 }), None);
        assert_eq!(c.feed(KeyEvent::ChordDown { at_ms: 60 }), None);
        assert_eq!(
            c.feed(KeyEvent::ChordUp { at_ms: 100 }),
            Some(HotkeyAction::TogglePanel)
        );
    }

    #[test]
    fn relachement_sans_appui_ignore() {
        let mut c = Classifier::new(250);
        assert_eq!(c.feed(KeyEvent::ChordUp { at_ms: 10 }), None);
    }
}
