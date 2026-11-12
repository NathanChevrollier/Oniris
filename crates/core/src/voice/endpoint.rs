//! Fin de parole : détecteur à énergie (RMS par trames de 30 ms, plancher de bruit adaptatif).
use std::collections::VecDeque;

pub const SPEECH_START_MS: usize = 200;
pub const END_SILENCE_MS: usize = 800;
pub const NO_SPEECH_MS: usize = 4000;
pub const MAX_UTTERANCE_MS: usize = 15000;

const FRAME_MS: usize = 30;
const FRAME: usize = 16 * FRAME_MS;
const SEED_FRAMES: usize = 300 / FRAME_MS;
const START_WINDOW_FRAMES: usize = 400 / FRAME_MS;
const WINDOW_FRAMES: usize = 1000 / FRAME_MS;
const MIN_THRESHOLD: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Continue,
    End,
    NoSpeech,
    TooLong,
}

pub struct Endpointer {
    rest: Vec<f32>,
    recent: VecDeque<f32>,
    frames: usize,
    flags: VecDeque<bool>,
    silence_run: usize,
    started: bool,
    done: Endpoint,
    event_ms: Option<u64>,
}

impl Default for Endpointer {
    fn default() -> Self {
        Self::new()
    }
}

impl Endpointer {
    pub fn new() -> Self {
        Self {
            rest: Vec::new(),
            recent: VecDeque::new(),
            frames: 0,
            flags: VecDeque::new(),
            silence_run: 0,
            started: false,
            done: Endpoint::Continue,
            event_ms: None,
        }
    }

    pub fn push(&mut self, samples_16k: &[f32]) -> Endpoint {
        if self.done != Endpoint::Continue {
            return self.done;
        }
        self.rest.extend_from_slice(samples_16k);
        let mut used = 0;
        while self.rest.len() - used >= FRAME {
            let frame = &self.rest[used..used + FRAME];
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt();
            used += FRAME;
            self.done = self.frame(rms);
            if self.done != Endpoint::Continue {
                self.event_ms = Some((self.frames * FRAME_MS) as u64);
                break;
            }
        }
        self.rest.drain(..used);
        self.done
    }

    /// Instant (ms d'audio, à la trame près) du premier événement.
    pub fn event_ms(&self) -> Option<u64> {
        self.event_ms
    }

    fn frame(&mut self, rms: f32) -> Endpoint {
        self.frames += 1;
        let ms = self.frames * FRAME_MS;
        let seeding = self.frames <= SEED_FRAMES;
        let floor = self.recent.iter().copied().fold(f32::MAX, f32::min);
        let speech = !seeding && rms > MIN_THRESHOLD.max(3.0 * floor);
        self.recent.push_back(rms);
        if self.recent.len() > WINDOW_FRAMES {
            self.recent.pop_front();
        }
        if ms >= MAX_UTTERANCE_MS {
            return Endpoint::TooLong;
        }
        // Début de parole : 200 ms de trames parlées dans les 400 ms écoulées (creux permis).
        self.flags.push_back(speech);
        if self.flags.len() > START_WINDOW_FRAMES {
            self.flags.pop_front();
        }
        if speech {
            self.silence_run = 0;
            if self.flags.iter().filter(|f| **f).count() * FRAME_MS >= SPEECH_START_MS {
                self.started = true;
            }
        } else {
            self.silence_run += 1;
        }
        if self.started {
            if self.silence_run * FRAME_MS >= END_SILENCE_MS {
                return Endpoint::End;
            }
        } else if ms >= NO_SPEECH_MS {
            return Endpoint::NoSpeech;
        }
        Endpoint::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % 20001) as f32 / 10000.0 - 1.0
        }
    }

    fn noise(rng: &mut Rng, secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * 16000.0) as usize).map(|_| rng.next() * amp).collect()
    }
    fn sine(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * 16000.0) as usize)
            .map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 16000.0).sin())
            .collect()
    }
    /// Parole syllabique : rafales de 240 ms séparées de 60 ms de bruit.
    fn speech(rng: &mut Rng, secs: f32, amp: f32, noise_amp: f32) -> Vec<f32> {
        let burst = sine(0.24, amp);
        let mut out = Vec::new();
        while out.len() < (secs * 16000.0) as usize {
            out.extend_from_slice(&burst);
            out.extend(noise(rng, 0.06, noise_amp));
        }
        out.truncate((secs * 16000.0) as usize);
        out
    }
    /// Alimente par morceaux ; renvoie l'événement et la position (en s) où il tombe.
    fn run(sig: &[f32], chunk: usize) -> (Endpoint, f32) {
        let mut e = Endpointer::new();
        for (i, c) in sig.chunks(chunk).enumerate() {
            let r = e.push(c);
            if r != Endpoint::Continue {
                return (r, ((i + 1) * chunk).min(sig.len()) as f32 / 16000.0);
            }
        }
        (Endpoint::Continue, sig.len() as f32 / 16000.0)
    }

    #[test]
    fn speech_then_silence_ends() {
        let mut r = Rng(1);
        let s = [noise(&mut r, 0.3, 0.005), speech(&mut r, 1.5, 0.2, 0.005), noise(&mut r, 1.0, 0.005)].concat();
        let (ev, at) = run(&s, 160);
        assert_eq!(ev, Endpoint::End);
        let after = at - 1.74; // dernière rafale finie à 0,3 + 1,44 s
        assert!((0.8..=0.9).contains(&after), "{after}");
    }
    #[test]
    fn nothing_said_is_no_speech() {
        let s = noise(&mut Rng(2), 5.0, 0.005);
        let (ev, at) = run(&s, 160);
        assert_eq!(ev, Endpoint::NoSpeech);
        assert!((at - 4.0).abs() <= 0.03, "{at}");
    }
    #[test]
    fn endless_speech_is_too_long() {
        let s = speech(&mut Rng(6), 16.0, 0.2, 0.005);
        let (ev, at) = run(&s, 160);
        assert_eq!(ev, Endpoint::TooLong);
        assert!((at - 15.0).abs() <= 0.03, "{at}");
    }
    #[test]
    fn short_click_is_not_speech() {
        let mut r = Rng(3);
        let s = [noise(&mut r, 2.0, 0.005), sine(0.1, 0.2), noise(&mut r, 2.9, 0.005)].concat();
        assert_eq!(run(&s, 160).0, Endpoint::NoSpeech);
    }
    #[test]
    fn loud_room_still_ends() {
        let mut r = Rng(4);
        let s = [noise(&mut r, 0.5, 0.05), sine(1.5, 0.4), noise(&mut r, 1.5, 0.05)].concat();
        assert_eq!(run(&s, 160).0, Endpoint::End);
    }
    #[test]
    fn chunk_size_does_not_matter() {
        let mut r = Rng(5);
        let s = [noise(&mut r, 0.3, 0.005), sine(1.5, 0.2), noise(&mut r, 1.5, 0.005)].concat();
        let ms = |chunk: usize| {
            let mut e = Endpointer::new();
            let ev = s.chunks(chunk).map(|c| e.push(c)).find(|r| *r != Endpoint::Continue);
            (ev, e.event_ms().unwrap() as i64)
        };
        let (a, ta) = ms(160);
        let (b, tb) = ms(4800);
        assert_eq!(a, b);
        assert!((ta - tb).abs() <= 30, "{ta} {tb}");
    }
    #[test]
    fn noise_rise_mid_listen_still_ends() {
        let mut r = Rng(7);
        let s = [noise(&mut r, 0.3, 0.005), speech(&mut r, 1.5, 0.2, 0.005), noise(&mut r, 4.0, 0.05)].concat();
        assert_eq!(run(&s, 160).0, Endpoint::End);
    }
    #[test]
    fn choppy_speech_starts() {
        let mut r = Rng(9);
        let mut s = Vec::new();
        while s.len() < 24000 {
            s.extend(sine(0.12, 0.2));
            s.extend(noise(&mut r, 0.08, 0.005));
        }
        s.truncate(24000);
        s.extend(noise(&mut r, 1.0, 0.005));
        assert_eq!(run(&s, 160).0, Endpoint::End);
    }
    #[test]
    fn quiet_speech_from_start_is_heard() {
        let mut r = Rng(8);
        // sinus 0.07 de crête : RMS ≈ 0.05
        let s = [speech(&mut r, 1.5, 0.07, 0.005), noise(&mut r, 1.5, 0.005)].concat();
        assert_eq!(run(&s, 160).0, Endpoint::End);
    }
}
