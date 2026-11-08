//! Sortie audio (cpal/WASAPI) derrière un trait injectable. Le flux vit sur son propre thread (`Stream` n'est pas `Send`) ;
//! lâcher le `Sink` coupe le son aussitôt. Journal : jamais de nom de périphérique.
#![allow(dead_code)] // consommé par le pipeline vocal (tâche suivante) ; à retirer alors
use super::piper::TtsError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};

/// Ouvre une sortie PCM 16 bits mono à `rate` Hz.
pub trait AudioOut: Send + Sync {
    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, TtsError>;
}

/// Une sortie ouverte ; son drop coupe le son sans attendre la fin de la file.
pub trait Sink: Send {
    fn push(&mut self, pcm: &[i16]);
    /// Échantillons poussés mais pas encore joués.
    fn pending(&self) -> usize;
}

pub struct CpalOut;

type Queue = Arc<Mutex<VecDeque<i16>>>;

struct CpalSink {
    queue: Queue,
    stop: mpsc::Sender<()>,
}

impl Sink for CpalSink {
    fn push(&mut self, pcm: &[i16]) {
        self.queue.lock().extend(pcm);
    }
    fn pending(&self) -> usize {
        self.queue.lock().len()
    }
}

impl Drop for CpalSink {
    fn drop(&mut self) {
        self.queue.lock().clear();
        let _ = self.stop.send(()); // le thread ferme le flux
    }
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, queue: Queue, rate: u32) -> Result<cpal::Stream, TtsError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let step = rate as f32 / config.sample_rate as f32;
    let mut frac = 0f32;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut q = queue.lock();
                for frame in data.chunks_mut(channels) {
                    // Interpolation linéaire entre deux échantillons voisins ; silence quand la file est vide.
                    let v = match (q.front(), q.get(1)) {
                        (Some(&a), Some(&b)) => (a as f32 + (b as f32 - a as f32) * frac) / 32768.0,
                        (Some(&a), None) => a as f32 / 32768.0,
                        _ => 0.0,
                    };
                    frame.fill(T::from_sample(v));
                    frac += step;
                    while frac >= 1.0 {
                        frac -= 1.0;
                        q.pop_front();
                    }
                }
            },
            |e| tracing::warn!("erreur du flux de sortie : {}", e.kind()),
            None,
        )
        .map_err(|e| TtsError::Audio(e.to_string()))
}

fn open_stream(queue: Queue, rate: u32) -> Result<cpal::Stream, TtsError> {
    let dev = cpal::default_host().default_output_device().ok_or_else(|| TtsError::Audio("aucune sortie audio".into()))?;
    let supported = dev.default_output_config().map_err(|e| TtsError::Audio(e.to_string()))?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&dev, config, queue, rate)?,
        cpal::SampleFormat::I16 => build::<i16>(&dev, config, queue, rate)?,
        cpal::SampleFormat::U16 => build::<u16>(&dev, config, queue, rate)?,
        cpal::SampleFormat::I32 => build::<i32>(&dev, config, queue, rate)?,
        other => return Err(TtsError::Audio(format!("format audio non géré ({other})"))),
    };
    stream.play().map_err(|e| TtsError::Audio(e.to_string()))?;
    Ok(stream)
}

impl AudioOut for CpalOut {
    fn open(&self, rate: u32) -> Result<Box<dyn Sink>, TtsError> {
        let queue: Queue = Arc::default();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let q = queue.clone();
        std::thread::spawn(move || match open_stream(q, rate) {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv(); // arrêt demandé, ou Sink lâché
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        });
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(CpalSink { queue, stop: stop_tx })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TtsError::Audio("le thread de sortie s'est arrêté".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vrai haut-parleur : jamais lancé par défaut (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn vrai_haut_parleur_un_court_bip() {
        let mut s = CpalOut.open(22_050).expect("sortie");
        let beep: Vec<i16> = (0..11_025).map(|i| ((i as f32 * 0.1).sin() * 3000.0) as i16).collect();
        s.push(&beep);
        while s.pending() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
