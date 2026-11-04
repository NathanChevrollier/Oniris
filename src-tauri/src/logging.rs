use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::filter::{LevelFilter, Targets};

/// `%APPDATA%\Oniris\logs`.
pub fn log_dir() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    appdata.join("Oniris").join("logs")
}

/// Nos crates en debug (dev) ou info (release), les dépendances (tao, reqwest…) en warn seulement.
fn targets(debug: bool) -> Targets {
    let ours = if debug { LevelFilter::DEBUG } else { LevelFilter::INFO };
    Targets::new()
        .with_targets([("oniris", ours), ("oniris_lib", ours), ("oniris_core", ours)])
        // Leurs journaux d'erreur contiennent des chemins de dossiers (contenu utilisateur).
        .with_targets([("notify", LevelFilter::OFF), ("ignore", LevelFilter::OFF)])
        .with_default(LevelFilter::WARN)
}

/// Installe le journal fichier (rotation quotidienne, 7 fichiers). Le garde doit rester vivant.
/// `None` si le dossier ou le subscriber ne peuvent pas être installés : l'app continue sans journal.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("oniris.log")
        .max_log_files(7)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = targets(cfg!(debug_assertions));
    let file = tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false);
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};
    let stderr = cfg!(debug_assertions).then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(file.with_filter(filter.clone()))
        .with(stderr.map(|l| l.with_filter(filter)))
        .try_init()
        .ok()?;
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_cree_le_dossier_et_ecrit() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let guard = init(&dir).expect("init");
        tracing::info!("bonjour");
        drop(guard);
        let ok = std::fs::read_dir(&dir).unwrap().any(|e| {
            let p = e.unwrap().path();
            p.file_name().unwrap().to_string_lossy().starts_with("oniris.log.")
                && std::fs::read_to_string(p).unwrap().contains("bonjour")
        });
        assert!(ok);
    }

    #[test]
    fn filtre_par_cible() {
        use tracing::Level;
        let dbg = targets(true);
        assert!(dbg.would_enable("oniris_lib::modules::weather", &Level::DEBUG));
        assert!(dbg.would_enable("oniris_core::tools::broker", &Level::DEBUG));
        assert!(!dbg.would_enable("reqwest::connect", &Level::INFO));
        assert!(dbg.would_enable("reqwest::connect", &Level::WARN));
        let rel = targets(false);
        assert!(rel.would_enable("oniris_lib::tray", &Level::INFO));
        assert!(!rel.would_enable("oniris_core::db", &Level::DEBUG));
        assert!(!rel.would_enable("tao::platform_impl", &Level::INFO));
        assert!(rel.would_enable("tao::platform_impl", &Level::WARN));
    }
}
