//! Job Object Windows : fermer le `Job` tue tout l'arbre du processus (réutilisé par le lanceur Claude Code).
use std::io;
use tokio::process::{Child, Command};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Poignée du Job Object ; son drop tue le processus et ses descendants.
pub struct Job(isize);

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: poignée créée par `spawn`, fermée une seule fois ici ; KILL_ON_JOB_CLOSE tue l'arbre.
        unsafe {
            let _ = CloseHandle(HANDLE(self.0 as *mut _));
        }
    }
}

/// Lance `cmd` sans console, dans un nouveau Job Object (`kill_on_drop` aussi, au cas où l'attache échoue).
// shortcut: le processus tourne un court instant avant d'être attaché (un petit-fils lancé avant échapperait), upgrade : CREATE_SUSPENDED si un lanceur le requiert.
pub fn spawn(mut cmd: Command) -> io::Result<(Child, Job)> {
    cmd.creation_flags(CREATE_NO_WINDOW).kill_on_drop(true);
    let child = cmd.spawn()?;
    let process = child.raw_handle().ok_or_else(|| io::Error::other("processus déjà terminé"))?;
    // SAFETY: appels Win32 sur une poignée de job fraîche et la poignée vivante du processus ; `info` vit pendant l'appel.
    unsafe {
        let h = CreateJobObjectW(None, windows::core::PCWSTR::null()).map_err(io::Error::other)?;
        let job = Job(h.0 as isize);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(h, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size_of_val(&info) as u32)
            .map_err(io::Error::other)?;
        AssignProcessToJobObject(h, HANDLE(process as *mut _)).map_err(io::Error::other)?;
        Ok((child, job))
    }
}
