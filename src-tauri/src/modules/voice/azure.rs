//! Voix Azure AI Speech (voix neurales de Microsoft) : la réponse part en SSML vers la région choisie, le PCM 24 kHz
//! revient et se joue sur la même sortie que Piper. Sans clé, hors ligne ou refusée : Piper prend le relais.
//! Journal : longueurs, durées et statut HTTP, jamais le texte ni la clé.
use super::piper::{SpeakHandle, Speaker, TtsError};
use super::playback::AudioOut;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Clé du coffre Windows.
pub const KEY_SECRET: &str = "azure-speech-key";
const RATE: u32 = 24_000;
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct AzureConf { pub key: String, pub region: String, pub voice: String }

pub struct AzureSpeaker {
    fallback: Arc<dyn Speaker>,
    out: Arc<dyn AudioOut>,
    http: reqwest::Client,
    /// `None` : voix Azure coupée ou sans clé.
    conf: Box<dyn Fn() -> Option<AzureConf> + Send + Sync>,
    current: Mutex<Option<SpeakHandle>>,
}

impl AzureSpeaker {
    pub fn new(fallback: Arc<dyn Speaker>, out: Arc<dyn AudioOut>, conf: Box<dyn Fn() -> Option<AzureConf> + Send + Sync>) -> Self {
        Self { fallback, out, http: reqwest::Client::new(), conf, current: Mutex::new(None) }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub fn ssml(text: &str, voice: &str) -> String {
    format!("<speak version='1.0' xml:lang='fr-FR'><voice name='{}'>{}</voice></speak>", escape(voice), escape(text))
}

/// PCM 16 bits mono 24 kHz de `text`.
pub async fn synthesize(http: &reqwest::Client, c: &AzureConf, text: &str) -> Result<Vec<i16>, String> {
    let url = format!("https://{}.tts.speech.microsoft.com/cognitiveservices/v1", c.region);
    let resp = http.post(url).timeout(TIMEOUT)
        .header("Ocp-Apim-Subscription-Key", &c.key)
        .header("Content-Type", "application/ssml+xml")
        .header("X-Microsoft-OutputFormat", "raw-24khz-16bit-mono-pcm")
        .header("User-Agent", "Oniris")
        .body(ssml(text, &c.voice))
        .send().await.map_err(|e| if e.is_timeout() { "Azure ne répond pas".to_string() } else { "Azure injoignable".to_string() })?;
    match resp.status().as_u16() {
        200 => {}
        401 | 403 => return Err("clé Azure refusée".into()),
        429 => return Err("quota Azure atteint".into()),
        s => return Err(format!("Azure a répondu {s}")),
    }
    let bytes = resp.bytes().await.map_err(|_| "réponse Azure coupée".to_string())?;
    Ok(bytes.as_chunks::<2>().0.iter().map(|&b| i16::from_le_bytes(b)).collect())
}

#[async_trait::async_trait]
impl Speaker for AzureSpeaker {
    fn stop(&self) {
        if let Some(h) = self.current.lock().take() {
            h.stop();
        }
        self.fallback.stop();
    }

    async fn speak(&self, text: &str, volume: u8) -> Result<SpeakHandle, TtsError> {
        self.stop();
        let Some(c) = (self.conf)() else { return self.fallback.speak(text, volume).await };
        let t = Instant::now();
        let pcm = match synthesize(&self.http, &c, text).await {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => return self.fallback.speak(text, volume).await,
            Err(e) => {
                tracing::warn!("voix Azure : {e}, repli sur Piper");
                return self.fallback.speak(text, volume).await;
            }
        };
        tracing::info!(chars = text.chars().count(), ms = t.elapsed().as_millis() as u64, "voix Azure reçue");
        let handle = play_pcm(self.out.clone(), pcm, volume).await?;
        *self.current.lock() = Some(handle.clone());
        Ok(handle)
    }
}

/// Joue le PCM 24 kHz ; `stop()` sur la poignée coupe le son restant.
async fn play_pcm(out: Arc<dyn AudioOut>, pcm: Vec<i16>, volume: u8) -> Result<SpeakHandle, TtsError> {
    let mut sink = tokio::task::spawn_blocking(move || out.open(RATE)).await.map_err(|e| TtsError::Start(e.to_string()))??;
    let (handle, done_tx) = SpeakHandle::new();
    let mut cancel = handle.cancel_rx();
    tokio::spawn(async move {
        let v = volume.min(100) as i32;
        let scaled: Vec<i16> = pcm.iter().map(|&s| (s as i32 * v / 100) as i16).collect();
        sink.push(&scaled);
        // Une sortie disparue ne se vide jamais : IRIS ne reste pas bloquée en Speaking.
        let deadline = Instant::now() + Duration::from_millis(scaled.len() as u64 * 1000 / RATE as u64) + Duration::from_secs(2);
        while sink.pending() > 0 && Instant::now() < deadline {
            tokio::select! {
                _ = cancel.wait_for(|c| *c) => break,
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
        drop(sink); // coupe le son restant
        let _ = done_tx.send(true);
    });
    Ok(handle)
}

// --- Branchement Tauri ---

/// Réglages et clé du coffre ; `None` si la voix Azure est coupée ou sans clé.
pub fn conf_from(app: &tauri::AppHandle) -> Option<AzureConf> {
    use tauri::Manager;
    let st = app.state::<crate::state::AppState>();
    let s = st.settings.read();
    if !s.voice_azure {
        return None;
    }
    let key = st.secrets.get(KEY_SECRET)?;
    Some(AzureConf { key, region: s.voice_azure_region.clone(), voice: s.voice_azure_voice.clone() })
}

fn key_ok(k: &str) -> bool { (16..=128).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric()) }

/// Range la clé dans le coffre Windows (jamais dans les réglages ni le journal).
#[tauri::command]
pub fn voice_azure_key_set(state: tauri::State<crate::state::AppState>, key: String) -> Result<(), String> {
    let key = key.trim();
    if !key_ok(key) {
        return Err("Clé invalide : copie la « Clé 1 » de ta ressource Speech (lettres et chiffres).".into());
    }
    state.secrets.set(KEY_SECRET, key)
}

#[tauri::command]
pub fn voice_azure_key_status(state: tauri::State<crate::state::AppState>) -> bool { state.secrets.get(KEY_SECRET).is_some() }

#[tauri::command]
pub fn voice_azure_key_clear(state: tauri::State<crate::state::AppState>) { state.secrets.delete(KEY_SECRET) }

/// « Écouter » : dit une phrase avec la région et la voix données (même non enregistrées) ; l'erreur d'Azure remonte.
#[tauri::command]
pub async fn voice_azure_test(app: tauri::AppHandle, region: String, voice: String) -> Result<(), String> {
    use tauri::Manager;
    let probe = oniris_core::Settings { voice_azure_region: region.clone(), voice_azure_voice: voice.clone(), ..Default::default() };
    probe.validate()?;
    let (key, volume) = {
        let st = app.state::<crate::state::AppState>();
        let key = st.secrets.get(KEY_SECRET).ok_or("Ajoute d'abord ta clé Azure.")?;
        let volume = st.settings.read().voice_volume;
        (key, volume)
    };
    let c = AzureConf { key, region, voice };
    let pcm = synthesize(&reqwest::Client::new(), &c, "Bonjour ! Je suis IRIS. Voilà à quoi ressemble ma nouvelle voix.").await?;
    let h = play_pcm(Arc::new(super::playback::CpalOut), pcm, volume).await.map_err(|e| e.to_string())?;
    h.done().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cle_bornee() {
        assert!(key_ok(&"a1".repeat(16)));
        assert!(!key_ok("court") && !key_ok(&"a".repeat(129)) && !key_ok(&format!("{} ", "a".repeat(20))));
    }

    #[test]
    fn ssml_echappe_le_texte_et_la_voix() {
        let s = ssml("Tom & Léa <3 \"oui\"", "fr-FR-DeniseNeural");
        assert_eq!(s, "<speak version='1.0' xml:lang='fr-FR'><voice name='fr-FR-DeniseNeural'>Tom &amp; Léa &lt;3 &quot;oui&quot;</voice></speak>");
        assert!(!ssml("x", "a'b").contains("a'b"));
    }

    struct Fallback(Mutex<u32>);
    #[async_trait::async_trait]
    impl Speaker for Fallback {
        fn stop(&self) {}
        async fn speak(&self, _: &str, _: u8) -> Result<SpeakHandle, TtsError> {
            *self.0.lock() += 1;
            let (h, done) = SpeakHandle::new();
            let _ = done.send(true);
            Ok(h)
        }
    }
    struct NoOut;
    impl AudioOut for NoOut {
        fn open(&self, _: u32) -> Result<Box<dyn super::super::playback::Sink>, TtsError> { Err(TtsError::Audio("aucune".into())) }
    }

    #[tokio::test]
    async fn sans_cle_ou_injoignable_piper_prend_le_relais() {
        let fb = Arc::new(Fallback(Mutex::new(0)));
        let off = AzureSpeaker::new(fb.clone(), Arc::new(NoOut), Box::new(|| None));
        off.speak("Bonjour", 80).await.unwrap();
        // Région qui rend l'URL invalide : échec avant tout accès réseau.
        let bad = AzureSpeaker::new(fb.clone(), Arc::new(NoOut), Box::new(|| Some(AzureConf { key: "k".into(), region: "pas une région".into(), voice: "v".into() })));
        bad.speak("Bonjour", 80).await.unwrap();
        assert_eq!(*fb.0.lock(), 2);
    }
}
