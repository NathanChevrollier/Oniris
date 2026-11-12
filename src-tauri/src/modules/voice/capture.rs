//! Capture du micro (cpal/WASAPI) derrière un trait. Le flux vit sur son propre thread (`Stream` n'est pas `Send`)
//! et n'est ouvert que le temps de l'enregistrement. Journal : jamais de nom de périphérique.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, Sample, SizedSample};
use oniris_core::voice::audio::{self, Resampler16k};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Durée maximale gardée ; la suite est ignorée.
const MAX_SECS: usize = 30;

#[derive(Debug, PartialEq)]
pub enum MicError {
    NoDevice,
    Denied,
    Other(String),
}

impl std::fmt::Display for MicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => write!(f, "Aucun micro n'est disponible."),
            Self::Denied => write!(f, "L'accès au micro est refusé : autorise-le dans les paramètres de confidentialité de Windows."),
            Self::Other(e) => write!(f, "Le micro n'a pas pu démarrer : {e}"),
        }
    }
}

impl From<cpal::Error> for MicError {
    fn from(e: cpal::Error) -> Self {
        match e.kind() {
            ErrorKind::PermissionDenied => Self::Denied,
            ErrorKind::DeviceNotAvailable => Self::NoDevice,
            _ => Self::Other(e.to_string()),
        }
    }
}

pub trait Mic: Send + Sync {
    /// Ouvre le micro `device` (vide ou introuvable : celui par défaut) ; `on_level` reçoit le niveau RMS, 30 fois/s au plus.
    fn start(&self, device: &str, on_level: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Recording>, MicError>;
    /// Flux continu : `sink` reçoit du mono 16 kHz, par morceaux quelconques, sur le thread audio (ne pas bloquer).
    fn listen(&self, device: &str, sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn Listening>, MicError>;
}

pub trait Listening: Send {
    /// Le micro est fermé au retour.
    fn stop(self: Box<Self>);
    /// Vrai si le flux a signalé une erreur (périphérique perdu).
    fn failed(&self) -> bool;
}

pub trait Recording: Send {
    /// Ferme le micro puis rend l'enregistrement en 16 kHz mono.
    fn stop(self: Box<Self>) -> Vec<f32>;
}

/// Échantillons natifs entrelacés, plafonnés à 30 s.
struct Buffer {
    samples: Vec<f32>,
    max: usize,
}

impl Buffer {
    fn push(&mut self, data: &[f32]) {
        let room = self.max.saturating_sub(self.samples.len());
        self.samples.extend_from_slice(&data[..data.len().min(room)]);
    }
}

pub struct CpalMic;

struct CpalRecording {
    stop: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<Vec<f32>>,
}

impl Recording for CpalRecording {
    fn stop(self: Box<Self>) -> Vec<f32> {
        let _ = self.stop.send(());
        // Le thread ferme le flux avant de rendre les échantillons.
        self.thread.join().unwrap_or_default()
    }
}

fn find_device(host: &cpal::Host, name: &str) -> Result<cpal::Device, MicError> {
    if !name.is_empty() {
        let found = host.input_devices()?.find(|d| d.description().is_ok_and(|x| x.name() == name));
        match found {
            Some(d) => return Ok(d),
            None => tracing::warn!("micro choisi introuvable, repli sur le micro par défaut"),
        }
    }
    host.default_input_device().ok_or(MicError::NoDevice)
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, buf: Arc<Mutex<Buffer>>, on_level: Box<dyn Fn(f32) + Send>) -> Result<cpal::Stream, MicError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut tmp: Vec<f32> = Vec::new(); // réutilisé : pas d'allocation par appel une fois chauffé
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            tmp.clear();
            tmp.extend(data.iter().map(|s| f32::from_sample(*s)));
            buf.lock().push(&tmp);
            if last.elapsed() >= Duration::from_millis(34) {
                last = Instant::now();
                on_level(audio::rms(&tmp));
            }
        },
        |e| tracing::warn!("erreur du flux micro : {}", e.kind()),
        None,
    )?;
    Ok(stream)
}

fn build_listen<T>(device: &cpal::Device, config: cpal::StreamConfig, mut rs: Resampler16k, mut sink: Box<dyn FnMut(&[f32]) + Send>, failed: Arc<AtomicBool>) -> Result<cpal::Stream, MicError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let (mut tmp, mut out): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new()); // réutilisés
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            tmp.clear();
            tmp.extend(data.iter().map(|s| f32::from_sample(*s)));
            out.clear();
            rs.push(&tmp, &mut out);
            sink(&out);
        },
        move |e| {
            failed.store(true, Ordering::Relaxed);
            tracing::warn!("erreur du flux micro : {}", e.kind());
        },
        None,
    )?;
    Ok(stream)
}

fn open_listen(device: &str, sink: Box<dyn FnMut(&[f32]) + Send>, failed: Arc<AtomicBool>) -> Result<cpal::Stream, MicError> {
    let host = cpal::default_host();
    let dev = find_device(&host, device)?;
    let supported = dev.default_input_config()?;
    let rs = Resampler16k::new(supported.sample_rate(), supported.channels());
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_listen::<f32>(&dev, config, rs, sink, failed)?,
        cpal::SampleFormat::I16 => build_listen::<i16>(&dev, config, rs, sink, failed)?,
        cpal::SampleFormat::U16 => build_listen::<u16>(&dev, config, rs, sink, failed)?,
        cpal::SampleFormat::I32 => build_listen::<i32>(&dev, config, rs, sink, failed)?,
        other => return Err(MicError::Other(format!("format audio non géré ({other})"))),
    };
    stream.play()?;
    Ok(stream)
}

struct CpalListening {
    stop: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
    failed: Arc<AtomicBool>,
}

impl Listening for CpalListening {
    fn stop(self: Box<Self>) {
        let _ = self.stop.send(());
        let _ = self.thread.join(); // le thread ferme le flux avant de finir
    }
    fn failed(&self) -> bool { self.failed.load(Ordering::Relaxed) }
}

type Opened = (cpal::Stream, Arc<Mutex<Buffer>>, u32, u16); // flux, tampon, fréquence, canaux

fn open(device: &str, on_level: Box<dyn Fn(f32) + Send>) -> Result<Opened, MicError> {
    let host = cpal::default_host();
    let dev = find_device(&host, device)?;
    let supported = dev.default_input_config()?;
    let (rate, channels, format) = (supported.sample_rate(), supported.channels(), supported.sample_format());
    let max = MAX_SECS * rate as usize * channels as usize;
    let buf = Arc::new(Mutex::new(Buffer { samples: Vec::new(), max }));
    let config = supported.config();
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&dev, config, buf.clone(), on_level)?,
        cpal::SampleFormat::I16 => build::<i16>(&dev, config, buf.clone(), on_level)?,
        cpal::SampleFormat::U16 => build::<u16>(&dev, config, buf.clone(), on_level)?,
        cpal::SampleFormat::I32 => build::<i32>(&dev, config, buf.clone(), on_level)?,
        other => return Err(MicError::Other(format!("format audio non géré ({other})"))),
    };
    stream.play()?;
    Ok((stream, buf, rate, channels))
}

impl Mic for CpalMic {
    fn start(&self, device: &str, on_level: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Recording>, MicError> {
        let device = device.to_string();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || match open(&device, on_level) {
            Ok((stream, buf, rate, channels)) => {
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv(); // arrêt demandé, ou Recording lâché
                drop(stream);
                let samples = std::mem::take(&mut buf.lock().samples);
                audio::resample_to_16k(&samples, rate, channels)
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                Vec::new()
            }
        });
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(CpalRecording { stop: stop_tx, thread })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(MicError::Other("le thread du micro s'est arrêté".into())),
        }
    }

    fn listen(&self, device: &str, sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn Listening>, MicError> {
        let device = device.to_string();
        let failed = Arc::new(AtomicBool::new(false));
        let f = failed.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || match open_listen(&device, sink, f) {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv();
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        });
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(CpalListening { stop: stop_tx, thread, failed })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(MicError::Other("le thread du micro s'est arrêté".into())),
        }
    }
}

/// Noms des micros (n'ouvre aucun flux).
#[tauri::command]
pub async fn voice_devices() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let host = cpal::default_host();
        host.input_devices()
            .map(|it| it.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoMic;
    impl Mic for NoMic {
        fn start(&self, _: &str, _: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Recording>, MicError> { Err(MicError::NoDevice) }
        fn listen(&self, _: &str, _: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn Listening>, MicError> { Err(MicError::NoDevice) }
    }

    #[test]
    fn un_faux_micro_sans_peripherique_remonte_no_device() {
        let err = NoMic.start("", Box::new(|_| {})).err().unwrap();
        assert_eq!(err, MicError::NoDevice);
        assert!(err.to_string().contains("micro"));
    }

    #[test]
    fn le_tampon_plafonne_a_30_secondes() {
        let (rate, channels) = (16_000usize, 1usize);
        let mut b = Buffer { samples: Vec::new(), max: MAX_SECS * rate * channels };
        for _ in 0..35 { b.push(&vec![0.1; rate]); }
        assert_eq!(b.samples.len(), 30 * 16_000);
    }

    #[test]
    fn les_erreurs_cpal_sont_classees() {
        assert_eq!(MicError::from(cpal::Error::new(ErrorKind::PermissionDenied)), MicError::Denied);
        assert_eq!(MicError::from(cpal::Error::new(ErrorKind::DeviceNotAvailable)), MicError::NoDevice);
    }

    // Vrai matériel : jamais lancé par défaut (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn vrai_micro_une_demi_seconde() {
        let rec = CpalMic.start("", Box::new(|_| {})).expect("micro");
        std::thread::sleep(Duration::from_millis(500));
        assert!(rec.stop().len() > 4_000);
    }

    #[test]
    #[ignore]
    fn vrai_micro_flux_deux_secondes() {
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let l = CpalMic.listen("", Box::new(move |s| { n2.fetch_add(s.len(), Ordering::Relaxed); })).expect("micro");
        std::thread::sleep(Duration::from_secs(2));
        assert!(!l.failed());
        l.stop();
        assert!(n.load(Ordering::Relaxed) >= 30_000);
    }
}
