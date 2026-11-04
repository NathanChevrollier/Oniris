//! État du PC. L'échantillonneur ne tourne que panneau ouvert : `start` à l'ouverture, `stop` à la fermeture.
//! CPU/RAM : sysinfo (rafraîchissements minimaux, une seule instance). GPU/VRAM : PDH (compteurs anglais, indépendants de la langue)
//! + DXGI pour la VRAM totale. Mémoire d'Oniris : octets privés (`PrivateUsage`) d'oniris.exe et de ses `msedgewebview2.exe`
//!   descendants, via Toolhelp32 + GetProcessMemoryInfo, recalculée toutes les 5 s.
use oniris_core::system::{gpu_from_counters, SystemStatus, NOT_INSTALLED};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tauri::{AppHandle, Emitter};

const MB: u64 = 1024 * 1024;
const TICK: Duration = Duration::from_secs(1);
const ONIRIS_EVERY: Duration = Duration::from_secs(5);

/// Machine à états pure de l'échantillonneur : chaque `start` invalide les tâches précédentes.
pub struct SamplerCtl { generation: AtomicU64 }

impl SamplerCtl {
    pub const fn new() -> Self { Self { generation: AtomicU64::new(0) } }
    /// Nouvelle génération : la tâche qui la reçoit tourne tant qu'elle reste courante.
    pub fn start(&self) -> u64 { self.generation.fetch_add(1, Ordering::SeqCst) + 1 }
    /// Invalide la génération courante (les ticks en cours sortent).
    pub fn stop(&self) { self.generation.fetch_add(1, Ordering::SeqCst); }
    pub fn is_current(&self, generation: u64) -> bool { self.generation.load(Ordering::SeqCst) == generation }
}

static CTL: SamplerCtl = SamplerCtl::new();

/// Appelé par `windows::{open_panel, close_panel}`.
pub fn set_sampling(app: &AppHandle, open: bool) {
    if !open {
        return CTL.stop();
    }
    let generation = CTL.start();
    let app = app.clone();
    std::thread::spawn(move || {
        let mut s = Sampler::new();
        s.prime(); // amorce : le CPU et les compteurs PDH ont besoin de deux lectures
        if !CTL.is_current(generation) {
            return; // fermé pendant l'amorce
        }
        while sleep_while_current(&CTL, generation, TICK) {
            let st = s.sample();
            if !CTL.is_current(generation) {
                break; // fermé pendant l'échantillon : rien n'est émis
            }
            if let Err(e) = app.emit_to("panel", "oniris://system", st) {
                tracing::warn!("événement system : {e}");
            }
        }
    });
}

/// Dort `total` par tranches de 100 ms ; `false` si la génération a changé entre-temps.
fn sleep_while_current(ctl: &SamplerCtl, generation: u64, total: Duration) -> bool {
    let end = Instant::now() + total;
    while Instant::now() < end {
        if !ctl.is_current(generation) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    ctl.is_current(generation)
}

/// Un instantané à la demande : deux lectures à 200 ms d'écart.
pub fn snapshot() -> SystemStatus {
    let mut s = Sampler::new();
    s.prime();
    std::thread::sleep(Duration::from_millis(200));
    s.sample()
}

#[tauri::command]
pub async fn system_snapshot() -> Result<SystemStatus, String> {
    tauri::async_runtime::spawn_blocking(snapshot).await.map_err(|e| e.to_string())
}

/// Désactive PDH après `limit` échecs de lecture consécutifs (une erreur isolée ne coupe pas le GPU de la session).
struct FailureCounter { consecutive: u32, limit: u32 }

impl FailureCounter {
    const fn new(limit: u32) -> Self { Self { consecutive: 0, limit } }
    /// `true` : garder PDH actif.
    fn record(&mut self, ok: bool) -> bool {
        self.consecutive = if ok { 0 } else { self.consecutive + 1 };
        self.consecutive < self.limit
    }
}

const PDH_FAILURE_LIMIT: u32 = 5;

struct Sampler {
    sys: System,
    gpu: Option<gpu::Pdh>,
    failures: FailureCounter,
    last_gpu: Option<(f32, u64)>,
    vram_total_mb: Option<u64>,
    oniris_mb: u64,
    oniris_at: Option<Instant>,
}

impl Sampler {
    fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        let gpu = gpu::Pdh::open().inspect_err(|e| tracing::warn!("PDH indisponible, GPU masqué : {e}")).ok();
        Self { sys, gpu, failures: FailureCounter::new(PDH_FAILURE_LIMIT), last_gpu: None, vram_total_mb: gpu::vram_total_mb(), oniris_mb: 0, oniris_at: None }
    }

    /// Première lecture : les erreurs PDH y sont normales (pas de point de comparaison), on les ignore.
    fn prime(&mut self) {
        self.read(false);
    }

    fn sample(&mut self) -> SystemStatus {
        self.read(true)
    }

    fn read(&mut self, count_failures: bool) -> SystemStatus {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        if self.oniris_at.is_none_or(|t| t.elapsed() >= ONIRIS_EVERY) {
            self.oniris_mb = procs::oniris_private_bytes() / MB;
            self.oniris_at = Some(Instant::now());
        }
        if let Some(pdh) = &self.gpu {
            match pdh.read() {
                Ok(Some(v)) => {
                    self.failures.record(true);
                    self.last_gpu = Some(v);
                }
                Ok(None) => {} // pas de donnée ce tick : on garde la dernière valeur
                Err(e) if count_failures => {
                    if !self.failures.record(false) {
                        tracing::warn!("PDH : {PDH_FAILURE_LIMIT} échecs de suite, GPU masqué ({e})");
                        self.gpu = None;
                        self.last_gpu = None;
                    }
                }
                Err(_) => {}
            }
        }
        let g = self.last_gpu;
        SystemStatus {
            cpu_pct: self.sys.global_cpu_usage().clamp(0.0, 100.0),
            ram_used_mb: self.sys.used_memory() / MB,
            ram_total_mb: self.sys.total_memory() / MB,
            gpu_pct: g.map(|g| g.0),
            vram_used_mb: g.map(|g| g.1),
            vram_total_mb: g.and(self.vram_total_mb),
            oniris_mb: self.oniris_mb,
            local_ai: NOT_INSTALLED.into(),
            voice: NOT_INSTALLED.into(),
        }
    }
}

/// Descendants de `root` dont l'exécutable est `exe`. Un passage par pid au plus : borné même si des pids sont réutilisés.
fn descendants_named(procs: &[(u32, u32, String)], root: u32, exe: &str) -> Vec<u32> {
    let mut seen: HashSet<u32> = HashSet::from([root]);
    let mut queue = vec![root];
    let mut out = vec![];
    while let Some(p) = queue.pop() {
        for (pid, _, name) in procs.iter().filter(|(_, parent, _)| *parent == p) {
            if seen.insert(*pid) {
                queue.push(*pid);
                if name.eq_ignore_ascii_case(exe) {
                    out.push(*pid);
                }
            }
        }
    }
    out
}

mod procs {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    /// (pid, pid parent, nom de l'exécutable) de tous les processus.
    fn all() -> Vec<(u32, u32, String)> {
        let mut v = vec![];
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return v };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            v.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
            ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
        }
        let _ = unsafe { CloseHandle(snap) };
        v
    }

    fn private_bytes(pid: u32) -> u64 {
        let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return 0 };
        let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
        let cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = unsafe { GetProcessMemoryInfo(h, std::ptr::addr_of_mut!(c).cast(), cb) }.is_ok();
        let _ = unsafe { CloseHandle(h) };
        if ok { c.PrivateUsage as u64 } else { 0 }
    }

    pub fn oniris_private_bytes() -> u64 {
        let me = std::process::id();
        let mut pids = super::descendants_named(&all(), me, "msedgewebview2.exe");
        pids.push(me);
        pids.into_iter().map(private_bytes).sum()
    }
}

mod gpu {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    use windows::Win32::System::Performance::*;

    /// VRAM dédiée de l'adaptateur le mieux doté.
    pub fn vram_total_mb() -> Option<u64> {
        let f: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        (0..)
            .map_while(|i| unsafe { f.EnumAdapters1(i) }.ok())
            .filter_map(|a| unsafe { a.GetDesc1() }.ok())
            .map(|d| d.DedicatedVideoMemory as u64 / MB)
            .max()
            .filter(|&m| m > 0)
    }

    pub struct Pdh { query: PDH_HQUERY, engine: PDH_HCOUNTER, mem: PDH_HCOUNTER }

    impl Drop for Pdh {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    fn check(status: u32, what: &str) -> Result<(), String> {
        if status == 0 { Ok(()) } else { Err(format!("{what} : 0x{status:08X}")) }
    }

    impl Pdh {
        /// Une requête par session d'échantillonnage.
        pub fn open() -> Result<Self, String> {
            let mut query = PDH_HQUERY::default();
            check(unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) }, "PdhOpenQuery")?;
            let mut p = Self { query, engine: PDH_HCOUNTER::default(), mem: PDH_HCOUNTER::default() };
            // En cas d'échec, `Drop` ferme la requête.
            check(unsafe { PdhAddEnglishCounterW(query, w!(r"\GPU Engine(*)\Utilization Percentage"), 0, &mut p.engine) }, "compteur GPU Engine")?;
            check(unsafe { PdhAddEnglishCounterW(query, w!(r"\GPU Adapter Memory(*)\Dedicated Usage"), 0, &mut p.mem) }, "compteur GPU Adapter Memory")?;
            Ok(p)
        }

        /// (utilisation %, VRAM utilisée en Mo).
        /// `Ok(None)` : pas de donnée ce tick (statuts transitoires de PDH).
        pub fn read(&self) -> Result<Option<(f32, u64)>, String> {
            let st = unsafe { PdhCollectQueryData(self.query) };
            if transient(st) {
                return Ok(None);
            }
            check(st, "PdhCollectQueryData")?;
            let (Some(engines), Some(mem)) = (array(self.engine)?, array(self.mem)?) else { return Ok(None) };
            let vram = mem.into_iter().map(|(_, v)| v).fold(0.0_f64, f64::max); // adaptateur le plus chargé
            Ok(Some((gpu_from_counters(&engines), (vram / MB as f64) as u64)))
        }
    }

    /// Statuts sans conséquence : première lecture, compteur sans dénominateur, aucune instance.
    fn transient(st: u32) -> bool {
        st == PDH_NO_DATA || st == PDH_INVALID_DATA || st == PDH_CALC_NEGATIVE_DENOMINATOR
    }

    /// `None` : pas de donnée ce tick. Boucle sur PDH_MORE_DATA (le nombre d'instances peut grandir entre deux appels).
    fn array(counter: PDH_HCOUNTER) -> Result<Option<Vec<(String, f64)>>, String> {
        let (mut size, mut count) = (0u32, 0u32);
        let mut buf: Vec<u64> = vec![]; // u64 pour l'alignement des éléments
        for _ in 0..4 {
            let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let arg = if buf.is_empty() { None } else { Some(items) };
            let st = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, arg) };
            if st == PDH_MORE_DATA {
                buf = vec![0u64; (size as usize).div_ceil(8)];
                continue;
            }
            if transient(st) {
                return Ok(None);
            }
            check(st, "PdhGetFormattedCounterArray")?;
            if buf.is_empty() {
                return Ok(Some(vec![]));
            }
            let slice = unsafe { std::slice::from_raw_parts(items, count as usize) };
            return Ok(Some(
                slice
                    .iter()
                    .filter(|i| i.FmtValue.CStatus == 0)
                    .map(|i| (unsafe { i.szName.to_string() }.unwrap_or_default(), unsafe { i.FmtValue.Anonymous.doubleValue }))
                    .collect(),
            ));
        }
        Err("PdhGetFormattedCounterArray : taille instable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pas_d_echantillonnage_apres_arret_ni_apres_redemarrage_obsolete() {
        let ctl = SamplerCtl::new();
        let g1 = ctl.start();
        assert!(ctl.is_current(g1));
        ctl.stop();
        assert!(!ctl.is_current(g1), "fermé : la tâche sort");
        let g2 = ctl.start();
        assert!(ctl.is_current(g2) && !ctl.is_current(g1), "rouvert : seule la nouvelle tâche tourne");
    }

    #[test]
    fn pdh_coupe_apres_cinq_echecs_consecutifs_seulement() {
        let mut f = FailureCounter::new(5);
        for _ in 0..4 {
            assert!(f.record(false));
        }
        assert!(f.record(true), "un succès remet à zéro");
        for _ in 0..4 {
            assert!(f.record(false));
        }
        assert!(!f.record(false), "5e échec de suite : PDH coupé");
    }

    #[test]
    fn la_pause_se_reveille_vite_a_l_arret() {
        let ctl = SamplerCtl::new();
        let g = ctl.start();
        ctl.stop();
        let t = Instant::now();
        assert!(!sleep_while_current(&ctl, g, Duration::from_secs(5)));
        assert!(t.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn descendants_filtres_et_bornes() {
        let p = |pid, parent, n: &str| (pid, parent, n.to_string());
        let procs = vec![
            p(1, 0, "oniris.exe"),
            p(2, 1, "msedgewebview2.exe"),
            p(3, 2, "MSEdgeWebView2.exe"),
            p(4, 1, "autre.exe"),
            p(5, 9, "msedgewebview2.exe"),
            p(1, 3, "oniris.exe"),
        ];
        let mut d = descendants_named(&procs, 1, "msedgewebview2.exe");
        d.sort();
        assert_eq!(d, vec![2, 3], "pas le processus d'un autre parent, pas de boucle sur pid réutilisé");
    }
}
