//! Banc de mesure : latence ouverture -> 2e requestAnimationFrame du panneau.
//! Compilé seulement en debug ou avec la feature `perf`.
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Debug, PartialEq)]
pub struct PerfStats {
    pub samples: u32,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

#[derive(Serialize)]
pub struct PerfReport {
    #[serde(flatten)]
    pub stats: PerfStats,
    /// Cycles sans `perf_frame` sous 2 s : exclus des statistiques.
    pub timeouts: u32,
}

/// Rang le plus proche (nearest-rank) sur un tableau trié. Tranche vide : 0.0.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Écart ouverture -> frame ; `None` si la frame précède l'ouverture (échantillon périmé).
fn delta_ms(open_at: u64, frame_at: u64) -> Option<f64> {
    frame_at.checked_sub(open_at).map(|d| d as f64)
}

// Présent seulement pendant un cycle armé : le panneau ne fait l'IPC que dans ce cas.
static WAITER: Mutex<Option<Sender<u64>>> = Mutex::new(None);

/// Appelée par le panneau (2e rAF après l'ouverture) quand `oniris://perf-armed` vaut true.
#[tauri::command]
pub fn perf_frame(at_ms: u64) {
    if let Some(tx) = WAITER.lock().take() {
        let _ = tx.send(at_ms);
    }
}

fn run_cycles(app: &AppHandle, n: u32) -> PerfReport {
    let mut deltas = Vec::new();
    let mut timeouts = 0;
    for _ in 0..n {
        let (tx, rx) = channel();
        *WAITER.lock() = Some(tx);
        // Instant juste avant l'ouverture (légèrement pessimiste vs l'`at_ms` de l'événement).
        let open_at = crate::windows::now_ms();
        if crate::windows::open_panel(app).is_err() {
            timeouts += 1;
            WAITER.lock().take();
            continue;
        }
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frame_at) => match delta_ms(open_at, frame_at) {
                Some(d) => deltas.push(d),
                None => timeouts += 1,
            },
            Err(_) => timeouts += 1,
        }
        WAITER.lock().take();
        let _ = crate::windows::close_panel(app);
        std::thread::sleep(Duration::from_millis(300));
    }
    deltas.sort_by(f64::total_cmp);
    PerfReport {
        stats: PerfStats {
            samples: deltas.len() as u32,
            p50_ms: percentile(&deltas, 0.5),
            p95_ms: percentile(&deltas, 0.95),
            max_ms: deltas.last().copied().unwrap_or(0.0),
        },
        timeouts,
    }
}

#[tauri::command]
pub async fn perf_open_cycles(app: AppHandle, n: u32) -> Result<PerfReport, String> {
    let _ = app.emit("oniris://perf-armed", true);
    let a = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || run_cycles(&a, n)).await;
    let _ = app.emit("oniris://perf-armed", false);
    res.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles() {
        let v = [1.0, 2.0, 3.0, 4.0, 100.0];
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 1.0), 100.0);
    }
    #[test]
    fn stale_frame_is_rejected() {
        assert_eq!(delta_ms(100, 140), Some(40.0));
        assert_eq!(delta_ms(100, 99), None);
    }
    #[test]
    fn percentile_empty() {
        assert_eq!(percentile(&[], 0.95), 0.0);
    }
}
