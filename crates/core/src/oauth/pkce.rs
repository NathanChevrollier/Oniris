//! PKCE (RFC 7636, méthode S256) et jetons aléatoires.
use base64::Engine;
use sha2::{Digest, Sha256};

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// `len` caractères non réservés (RFC 3986) tirés du générateur du système.
pub fn random(len: usize) -> String {
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut buf).expect("générateur aléatoire du système");
        // Rejet au-delà du plus grand multiple de 66 : pas de biais modulo.
        for b in buf.iter().filter(|&&b| (b as usize) < UNRESERVED.len() * 3) {
            if out.len() < len {
                out.push(UNRESERVED[*b as usize % UNRESERVED.len()] as char);
            }
        }
    }
    out
}

/// base64url(sha256(verifier)), sans remplissage.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// (verifier de 64 caractères non réservés, challenge S256).
pub fn new() -> (String, String) {
    let v = random(64);
    let c = challenge(&v);
    (v, c)
}

/// Paramètre `state` de la redirection.
pub fn state() -> String { random(32) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn vecteur_rfc_7636() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
    #[test] fn verifier_non_reserve_et_aleatoire() {
        let (v, c) = new();
        assert_eq!(v.len(), 64);
        assert!(v.bytes().all(|b| UNRESERVED.contains(&b)));
        assert_eq!(c, challenge(&v));
        assert!(!c.contains('='));
        assert_ne!(new().0, v);
        assert_ne!(state(), state());
    }
}
