//! Transcription par `whisper-server` local (HTTP) et nettoyage du texte. Jamais de texte transcrit dans le journal.
use reqwest::multipart::{Form, Part};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("voix non installée")]
    NotInstalled,
    #[error("démarrage de Whisper impossible : {0}")]
    Start(String),
    #[error("Whisper n'a pas répondu à temps")]
    Timeout,
    #[error("réponse de Whisper invalide : {0}")]
    Bad(String),
}

/// Envoie le WAV (16 kHz mono) à `{base}/inference` et renvoie le texte brut.
pub async fn transcribe(http: &reqwest::Client, base: &str, wav: Vec<u8>) -> Result<String, SttError> {
    transcribe_within(http, base, wav, Duration::from_secs(15)).await
}

async fn transcribe_within(http: &reqwest::Client, base: &str, wav: Vec<u8>, timeout: Duration) -> Result<String, SttError> {
    let file = Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav").map_err(|e| SttError::Bad(e.to_string()))?;
    let form = Form::new().part("file", file).text("language", "fr").text("response_format", "json").text("temperature", "0.0");
    let resp = http.post(format!("{base}/inference")).multipart(form).timeout(timeout).send().await.map_err(|e| {
        if e.is_timeout() { SttError::Timeout } else { SttError::Bad(e.without_url().to_string()) }
    })?;
    if !resp.status().is_success() {
        return Err(SttError::Bad(format!("HTTP {}", resp.status())));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| if e.is_timeout() { SttError::Timeout } else { SttError::Bad(e.to_string()) })?;
    v["text"].as_str().map(str::to_string).ok_or_else(|| SttError::Bad("champ text absent".into()))
}

const HALLUCINATIONS: [&str; 5] = ["sous-titres realises par", "sous-titrage", "merci d'avoir regarde", "abonnez-vous", "amara.org"];

/// Texte prêt à interpréter, ou `None` (silence, bruit, hallucination connue de Whisper).
pub fn clean(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '♪' | '"' | '«' | '»' | '“' | '”' => {}
            c => out.push(c),
        }
    }
    let out = out.trim().to_string();
    let key = crate::when::norm(&out);
    // Début de texte, ou crédit en fin de texte (« Merci. Sous-titres réalisés par… Amara.org ») :
    // « Active le sous-titrage » est une vraie phrase.
    let bad = key == "musique" || HALLUCINATIONS.iter().any(|h| key.starts_with(h)) || key.ends_with("amara.org");
    (out.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && !bad).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn transcribe_envoie_les_champs_et_lit_le_texte() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/inference"))
            .and(body_string_contains("name=\"file\"")).and(body_string_contains("filename=\"audio.wav\""))
            .and(body_string_contains("name=\"language\"\r\n\r\nfr")).and(body_string_contains("name=\"response_format\"\r\n\r\njson"))
            .and(body_string_contains("name=\"temperature\"\r\n\r\n0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": " Bonjour IRIS"}))).mount(&s).await;
        let t = transcribe(&reqwest::Client::new(), &s.uri(), vec![1, 2, 3]).await.unwrap();
        assert_eq!(t, " Bonjour IRIS");
    }

    #[tokio::test]
    async fn transcribe_500_est_bad_et_lenteur_est_timeout() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
        assert!(matches!(transcribe(&reqwest::Client::new(), &s.uri(), vec![0]).await, Err(SttError::Bad(_))));
        let s = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(20))).mount(&s).await;
        let r = transcribe_within(&reqwest::Client::new(), &s.uri(), vec![0], Duration::from_millis(200)).await;
        assert!(matches!(r, Err(SttError::Timeout)));
    }

    #[test]
    fn clean_filtre_le_bruit_et_les_hallucinations() {
        for bad in ["", "  ", "[Musique]", "(silence)", "♪", "a", "Sous-titres réalisés par la communauté d'Amara.org", "Merci d'avoir regardé cette vidéo !", "Abonnez-vous", "Musique", "Sous-titrage ST' 501"] {
            assert_eq!(clean(bad), None, "{bad}");
        }
        assert_eq!(clean("Mets la playlist Focus").as_deref(), Some("Mets la playlist Focus"));
        assert_eq!(clean(" [BLANK_AUDIO] \"Ouvre Spotify\" ♪").as_deref(), Some("Ouvre Spotify"));
        assert_eq!(clean("Merci. Sous-titres réalisés par la communauté d'Amara.org"), None);
        assert_eq!(clean("Active le sous-titrage").as_deref(), Some("Active le sous-titrage"));
        assert_eq!(clean("Mets de la musique").as_deref(), Some("Mets de la musique"));
    }
}
