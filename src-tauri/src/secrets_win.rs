//! Coffre Windows : gestionnaire d'identifiants, identifiants génériques `Oniris/<key>`, persistance locale à la machine
//! (protégée par le compte Windows, jamais itinérante). Aucune valeur n'est journalisée.
use oniris_core::secrets::SecretStore;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

pub struct WinCredentials;

fn target(key: &str) -> String { format!("Oniris/{key}") }

impl SecretStore for WinCredentials {
    fn get(&self, key: &str) -> Option<String> {
        let mut p: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY : `p` est rempli par CredReadW en cas de succès, lu puis libéré par CredFree.
        unsafe {
            CredReadW(&HSTRING::from(target(key)), CRED_TYPE_GENERIC, None, &mut p).ok()?;
            let c = &*p;
            let blob = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
            let v = String::from_utf8(blob.to_vec()).ok();
            CredFree(p as *const _);
            v
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let mut name: Vec<u16> = target(key).encode_utf16().chain([0]).collect();
        let mut blob = value.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(name.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY : `name` et `blob` vivent jusqu'à la fin de l'appel ; CredWriteW les copie.
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| format!("gestionnaire d'identifiants : {}", e.code()))
    }

    fn delete(&self, key: &str) {
        // SAFETY : simple appel avec une chaîne terminée par zéro.
        let _ = unsafe { CredDeleteW(&HSTRING::from(target(key)), CRED_TYPE_GENERIC, None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touche le vrai gestionnaire d'identifiants : à lancer à la main (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn vrai_coffre_aller_retour() {
        let key = format!("test-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let w = WinCredentials;
        assert_eq!(w.get(&key), None);
        w.set(&key, "valeur-é").unwrap();
        assert_eq!(w.get(&key).as_deref(), Some("valeur-é"));
        w.set(&key, "autre").unwrap();
        assert_eq!(w.get(&key).as_deref(), Some("autre"));
        w.delete(&key);
        assert_eq!(w.get(&key), None);
    }
}
