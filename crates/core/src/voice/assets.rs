//! Catalogue des fichiers de la voix (épinglés par URL et SHA-256), téléchargement vérifié et extraction sûre.
//! La confiance repose sur le SHA-256 : l'hôte final peut être un CDN après redirection. Rien n'est jamais exécuté ici.
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// Fichier posé tel quel à `dest` (relatif au dossier de la voix).
    File { dest: &'static str },
    /// Archive extraite dans `dest_dir`. Le premier dossier de chaque entrée (`Release/`, `piper/`) est retiré, le reste
    /// (sous-dossiers compris) est conservé ; `keep` : motifs (`*` = toute suite de caractères) sur le chemin dans l'archive.
    Zip { dest_dir: &'static str, keep: &'static [&'static str] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub key: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// Taille en octets, si épinglée.
    pub size: Option<u64>,
    pub kind: AssetKind,
}

pub const CATALOG: &[Asset] = &[
    Asset {
        key: "whisper-bin",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip",
        sha256: "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb",
        size: None,
        kind: AssetKind::Zip { dest_dir: "whisper", keep: &["Release/whisper-server.exe", "Release/whisper.dll", "Release/ggml*.dll"] },
    },
    Asset {
        key: "piper-bin",
        url: "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
        sha256: "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
        size: Some(22_477_236),
        kind: AssetKind::Zip { dest_dir: "piper", keep: &["piper/*"] },
    },
    Asset {
        key: "voice-onnx",
        url: "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx",
        sha256: "641d1ab097da2b81128c076810edb052b385decc8be3381814802a64a73baf99",
        size: Some(63_201_294),
        kind: AssetKind::File { dest: "models/fr_FR-siwis-medium.onnx" },
    },
    Asset {
        key: "voice-json",
        url: "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx.json",
        sha256: "39479916c2db192b5ac9764daddd0c744d83e023ad890c6976c0633ae4df8959",
        size: Some(4_875),
        kind: AssetKind::File { dest: "models/fr_FR-siwis-medium.onnx.json" },
    },
    Asset {
        key: "stt-small",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        size: Some(190_085_487),
        kind: AssetKind::File { dest: "models/ggml-small-q5_1.bin" },
    },
    Asset {
        key: "stt-turbo",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size: Some(574_041_195),
        kind: AssetKind::File { dest: "models/ggml-large-v3-turbo-q5_0.bin" },
    },
];

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("Téléchargement impossible : vérifie ta connexion puis réessaie.")]
    Network,
    #[error("Le fichier téléchargé ne correspond pas à l'empreinte attendue : il a été supprimé, réessaie.")]
    Hash,
    #[error("Archive refusée (chemin dangereux : {0}).")]
    Unsafe(String),
    #[error("Écriture impossible sur le disque : {0}")]
    Io(String),
}

impl From<std::io::Error> for AssetError {
    fn from(e: std::io::Error) -> Self { Self::Io(e.to_string()) }
}

/// Client dédié : redirections permises (GitHub et Hugging Face passent par des CDN), HTTPS seulement,
/// connexion 15 s, silence maximal de 60 s, pas de délai total (les modèles pèsent des centaines de Mo).
pub fn http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
}

fn is_wanted(key: &str, stt_model: &str) -> bool {
    match key {
        "stt-small" => stt_model != "turbo",
        "stt-turbo" => stt_model == "turbo",
        _ => true,
    }
}

/// Binaires, voix et seulement le modèle STT choisi.
pub fn needed(stt_model: &str) -> Vec<&'static Asset> { needed_in(CATALOG, stt_model) }

pub fn needed_in<'a>(catalog: &'a [Asset], stt_model: &str) -> Vec<&'a Asset> {
    catalog.iter().filter(|a| is_wanted(a.key, stt_model)).collect()
}

fn installed(a: &Asset, dir: &Path) -> bool {
    match a.kind {
        AssetKind::File { dest } => dir.join(dest).is_file(),
        // Le dossier n'apparaît que par renommage atomique d'une extraction complète.
        AssetKind::Zip { dest_dir, .. } => dir.join(dest_dir).is_dir(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VoiceStatus {
    pub installed: bool,
    pub missing: Vec<String>,
    /// Octets restant à télécharger (les tailles non épinglées comptent pour 0).
    pub bytes_missing: u64,
}

pub fn status(dir: &Path, stt_model: &str) -> VoiceStatus { status_in(CATALOG, dir, stt_model) }

pub fn status_in(catalog: &[Asset], dir: &Path, stt_model: &str) -> VoiceStatus {
    let missing: Vec<&Asset> = needed_in(catalog, stt_model).into_iter().filter(|a| !installed(a, dir)).collect();
    VoiceStatus {
        installed: missing.is_empty(),
        bytes_missing: missing.iter().filter_map(|a| a.size).sum(),
        missing: missing.iter().map(|a| a.key.to_string()).collect(),
    }
}

/// `*` = toute suite de caractères (vide comprise).
fn glob(pat: &str, s: &str) -> bool {
    match pat.split_once('*') {
        None => pat == s,
        Some((head, rest)) => s.strip_prefix(head).is_some_and(|t| (0..=t.len()).filter(|i| t.is_char_boundary(*i)).any(|i| glob(rest, &t[i..]))),
    }
}

/// Chemin d'une entrée d'archive, normalisé en `/` ; refuse absolu, `..`, `:` et `\\`.
fn safe_name(raw: &str) -> Result<String, AssetError> {
    let n = raw.replace('\\', "/");
    if n.starts_with('/') || n.contains(':') || n.split('/').any(|c| c == "..") {
        return Err(AssetError::Unsafe(raw.chars().take(60).collect()));
    }
    Ok(n)
}

/// Extrait `zip` dans un dossier voisin de `dest_dir`, puis le renomme : jamais d'extraction à moitié.
pub fn extract(zip: &Path, dest_dir: &Path, keep: &[&str]) -> Result<(), AssetError> {
    let io = |e: zip::result::ZipError| AssetError::Io(e.to_string());
    let mut ar = zip::ZipArchive::new(fs::File::open(zip)?).map_err(io)?;
    let mut names = Vec::new();
    for i in 0..ar.len() {
        names.push(safe_name(ar.by_index(i).map_err(io)?.name())?); // tout est contrôlé avant d'écrire quoi que ce soit
    }
    let tmp = dest_dir.with_extension("tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    let r = (|| -> Result<(), AssetError> {
        for (i, name) in names.iter().enumerate() {
            let mut f = ar.by_index(i).map_err(io)?;
            if f.is_dir() || !keep.iter().any(|p| glob(p, name)) { continue; }
            let Some((_, rel)) = name.split_once('/') else { continue };
            let out = rel.split('/').filter(|c| !c.is_empty()).fold(tmp.clone(), |p, c| p.join(c));
            if let Some(p) = out.parent() { fs::create_dir_all(p)?; }
            std::io::copy(&mut f, &mut fs::File::create(&out)?)?;
        }
        Ok(())
    })();
    if let Err(e) = r {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let _ = fs::remove_dir_all(dest_dir);
    fs::rename(&tmp, dest_dir).inspect_err(|_| { let _ = fs::remove_dir_all(&tmp); })?;
    Ok(())
}

/// Télécharge `asset` vers `dir`, vérifie le SHA-256 au fil de l'eau, puis pose le fichier (renommage atomique) ou extrait l'archive.
pub async fn download(
    http: &reqwest::Client,
    asset: &Asset,
    dir: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), AssetError> {
    let target: PathBuf = match asset.kind {
        AssetKind::File { dest } => dir.join(dest),
        AssetKind::Zip { dest_dir, .. } => dir.join(dest_dir),
    };
    let part = PathBuf::from(format!("{}.part", target.display()));
    if let Some(p) = part.parent() { fs::create_dir_all(p)?; }
    if let Err(e) = fetch(http, asset, &part, &mut on_progress).await {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    match asset.kind {
        AssetKind::File { .. } => {
            let _ = fs::remove_file(&target);
            fs::rename(&part, &target).inspect_err(|_| { let _ = fs::remove_file(&part); })?;
            Ok(())
        }
        AssetKind::Zip { keep, .. } => {
            let (z, t) = (part.clone(), target.clone());
            let r = tokio::task::spawn_blocking(move || extract(&z, &t, keep)).await.map_err(|e| AssetError::Io(e.to_string()))?;
            let _ = fs::remove_file(&part);
            r
        }
    }
}

async fn fetch(http: &reqwest::Client, asset: &Asset, part: &Path, on_progress: &mut impl FnMut(u64, Option<u64>)) -> Result<(), AssetError> {
    let mut resp = http.get(asset.url).send().await.and_then(|r| r.error_for_status()).map_err(|_| AssetError::Network)?;
    let total = resp.content_length().or(asset.size);
    let mut file = fs::File::create(part)?;
    let (mut hash, mut done) = (Sha256::new(), 0u64);
    while let Some(chunk) = resp.chunk().await.map_err(|_| AssetError::Network)? {
        hash.update(&chunk);
        file.write_all(&chunk)?;
        done += chunk.len() as u64;
        on_progress(done, total);
    }
    file.flush()?;
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if hex != asset.sha256 || asset.size.is_some_and(|s| s != done) { return Err(AssetError::Hash); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sha(b: &[u8]) -> String { Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() }
    fn leak(s: String) -> &'static str { Box::leak(s.into_boxed_str()) }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (n, d) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    async fn serve(body: Vec<u8>) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/f")).respond_with(ResponseTemplate::new(200).set_body_bytes(body)).mount(&s).await;
        s
    }

    fn file_asset(url: String, hash: String) -> Asset {
        Asset { key: "t", url: leak(url), sha256: leak(hash), size: None, kind: AssetKind::File { dest: "models/t.bin" } }
    }

    #[tokio::test]
    async fn hash_bon_le_fichier_est_en_place() {
        let body = b"bonjour".to_vec();
        let s = serve(body.clone()).await;
        let d = tempfile::tempdir().unwrap();
        let a = file_asset(format!("{}/f", s.uri()), sha(&body));
        let mut last = (0, None);
        download(&reqwest::Client::new(), &a, d.path(), |x, t| last = (x, t)).await.unwrap();
        assert_eq!(fs::read(d.path().join("models/t.bin")).unwrap(), body);
        assert!(!d.path().join("models/t.bin.part").exists());
        assert_eq!(last, (7, Some(7)));
    }

    #[tokio::test]
    async fn hash_faux_ni_fichier_ni_part() {
        let s = serve(b"bonjour".to_vec()).await;
        let d = tempfile::tempdir().unwrap();
        let a = file_asset(format!("{}/f", s.uri()), sha(b"autre"));
        let r = download(&reqwest::Client::new(), &a, d.path(), |_, _| {}).await;
        assert!(matches!(r, Err(AssetError::Hash)));
        assert_eq!(fs::read_dir(d.path().join("models")).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn coupure_au_milieu_pas_de_fichier_final() {
        use tokio::io::AsyncWriteExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/f", l.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut c, _) = l.accept().await.unwrap();
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nabc").await;
        });
        let d = tempfile::tempdir().unwrap();
        let a = file_asset(url, sha(b"x"));
        let r = download(&reqwest::Client::new(), &a, d.path(), |_, _| {}).await;
        assert!(matches!(r, Err(AssetError::Network)));
        assert_eq!(fs::read_dir(d.path().join("models")).unwrap().count(), 0);
    }

    #[test]
    fn zip_dangereux_refuse_sans_rien_ecrire() {
        for evil in ["../evil.txt", r"C:\x", r"\\srv\x", "/abs.txt", "a/../../b"] {
            let d = tempfile::tempdir().unwrap();
            let z = d.path().join("a.zip");
            fs::write(&z, zip_of(&[("Release/ok.dll", b"1"), (evil, b"2")])).unwrap();
            let dest = d.path().join("out");
            assert!(matches!(extract(&z, &dest, &["*"]), Err(AssetError::Unsafe(_))), "{evil}");
            assert!(!dest.exists() && !dest.with_extension("tmp").exists() && !d.path().join("evil.txt").exists());
        }
    }

    #[test]
    fn keep_filtre_et_aplatit_un_niveau() {
        let d = tempfile::tempdir().unwrap();
        let z = d.path().join("a.zip");
        fs::write(&z, zip_of(&[
            ("Release/whisper-server.exe", b"1"), ("Release/ggml-cpu.dll", b"2"), ("Release/bench.exe", b"3"),
            ("piper/espeak-ng-data/voices/x", b"4"),
        ])).unwrap();
        let dest = d.path().join("whisper");
        extract(&z, &dest, &["Release/whisper-server.exe", "Release/ggml*.dll"]).unwrap();
        let mut got: Vec<_> = fs::read_dir(&dest).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        got.sort();
        assert_eq!(got, ["ggml-cpu.dll", "whisper-server.exe"]);
        let p = d.path().join("piper");
        extract(&z, &p, &["piper/*"]).unwrap();
        assert!(p.join("espeak-ng-data/voices/x").is_file());
    }

    #[tokio::test]
    async fn zip_telecharge_puis_extrait() {
        let body = zip_of(&[("Release/whisper.dll", b"1")]);
        let s = serve(body.clone()).await;
        let d = tempfile::tempdir().unwrap();
        let a = Asset { key: "z", url: leak(format!("{}/f", s.uri())), sha256: leak(sha(&body)), size: None, kind: AssetKind::Zip { dest_dir: "whisper", keep: &["Release/whisper.dll"] } };
        download(&reqwest::Client::new(), &a, d.path(), |_, _| {}).await.unwrap();
        assert!(d.path().join("whisper/whisper.dll").is_file());
        assert!(!d.path().join("whisper.part").exists());
    }

    #[test]
    fn needed_ne_prend_que_le_modele_choisi() {
        let keys = |m| needed(m).iter().map(|a| a.key).collect::<Vec<_>>();
        assert!(!keys("turbo").contains(&"stt-small") && keys("turbo").contains(&"stt-turbo"));
        assert!(!keys("small").contains(&"stt-turbo") && keys("small").contains(&"stt-small"));
        assert_eq!(keys("small").len(), 5);
    }

    #[test]
    fn status_liste_les_manquants() {
        let d = tempfile::tempdir().unwrap();
        let st = status(d.path(), "small");
        assert!(!st.installed && st.missing.len() == 5);
        assert_eq!(st.bytes_missing, 22_477_236 + 63_201_294 + 4_875 + 190_085_487);
        fs::create_dir_all(d.path().join("models")).unwrap();
        fs::create_dir_all(d.path().join("whisper")).unwrap();
        fs::write(d.path().join("models/fr_FR-siwis-medium.onnx"), b"x").unwrap();
        let st = status(d.path(), "small");
        assert_eq!(st.missing, ["piper-bin", "voice-json", "stt-small"]);
    }

    #[test]
    fn catalogue_reel_epingle() {
        assert_eq!(CATALOG.len(), 6);
        for a in CATALOG {
            assert!(a.sha256.len() == 64 && a.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{}", a.key);
            let u = url::Url::parse(a.url).unwrap();
            assert_eq!(u.scheme(), "https");
            assert!(matches!(u.host_str(), Some("github.com" | "huggingface.co")), "{}", a.key);
        }
    }
}
