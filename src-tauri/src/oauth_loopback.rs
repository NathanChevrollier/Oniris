//! Redirection OAuth en boucle locale : écoute `127.0.0.1:47829` seulement, attend **une** requête
//! `GET /callback?code=…&state=…`, vérifie `state`. Toute autre requête reçoit un 404 et l'écoute continue.
use oniris_core::oauth::LOOPBACK_PORT;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PAGE_OK: &str = "<!doctype html><meta charset=\"utf-8\"><title>Oniris</title><body style=\"font-family:system-ui;text-align:center;padding:3em\"><h1>Compte connecté</h1><p>Compte connecté, tu peux fermer cet onglet.</p></body>";
const PAGE_KO: &str = "<!doctype html><meta charset=\"utf-8\"><title>Oniris</title><body style=\"font-family:system-ui;text-align:center;padding:3em\"><h1>Connexion refusée</h1><p>La connexion n'a pas abouti. Reviens dans Oniris pour réessayer.</p></body>";

/// Attend le code d'autorisation, `timeout` au plus (5 minutes en production).
pub async fn wait_code(state: &str, timeout: Duration) -> Result<String, String> {
    serve(bind(LOOPBACK_PORT).await?, state, timeout).await
}

/// Lie `127.0.0.1:port` ; erreurs en clair, sans essayer d'autre port.
async fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => format!("Le port {port} est déjà utilisé : ferme l'application qui l'occupe, puis réessaie."),
        std::io::ErrorKind::PermissionDenied => format!("Windows refuse l'écoute sur le port {port} (pare-feu ou plage réservée) : réessaie après un redémarrage."),
        _ => format!("Écoute locale impossible sur le port {port} ({:?}).", e.kind()),
    })
}

/// Cœur de `wait_code`, sur un écouteur déjà lié (les tests lient le port 0).
async fn serve(listener: TcpListener, state: &str, timeout: Duration) -> Result<String, String> {
    let wait = async {
        loop {
            let mut stream = match listener.accept().await {
                Ok((s, _)) => s,
                Err(_) => {
                    // Erreur passagère (connexion avortée…) : pas de boucle active.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            // Un client muet ne bloque pas l'écoute plus de 5 s.
            let Ok(Some(target)) = tokio::time::timeout(Duration::from_secs(5), request_target(&mut stream)).await else {
                respond(&mut stream, "404 Not Found", "").await;
                continue;
            };
            match callback(&target, state) {
                None => respond(&mut stream, "404 Not Found", "").await,
                Some(r) => {
                    respond(&mut stream, if r.is_ok() { "200 OK" } else { "400 Bad Request" }, if r.is_ok() { PAGE_OK } else { PAGE_KO }).await;
                    return r;
                }
            }
        }
    };
    tokio::time::timeout(timeout, wait).await.map_err(|_| "Délai dépassé : la connexion n'a pas été terminée dans le navigateur.".to_string())?
}

/// Cible de la ligne de requête `GET <cible> HTTP/1.1` ; `None` si illisible ou autre méthode.
async fn request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 || buf.len() > 8192 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = std::str::from_utf8(&buf).ok()?;
    let mut parts = head.lines().next()?.split(' ');
    (parts.next()? == "GET").then(|| parts.next().map(str::to_string))?
}

/// `None` : pas la redirection (404). `Some(Err)` : redirection refusée (state faux, erreur du fournisseur).
fn callback(target: &str, state: &str) -> Option<Result<String, String>> {
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    if get("state").as_deref() != Some(state) {
        return Some(Err("Réponse de connexion invalide (state) : réessaie.".into()));
    }
    if get("error").is_some() {
        return Some(Err("Connexion refusée dans le navigateur.".into()));
    }
    Some(get("code").filter(|c| !c.is_empty()).ok_or_else(|| "Réponse de connexion sans code : réessaie.".to_string()))
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(msg.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn listener() -> (TcpListener, u16) {
        let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn bon_code_renvoye_apres_un_404() {
        let (l, port) = listener().await;
        let task = tokio::spawn(async move { serve(l, "s3cr3t", Duration::from_secs(10)).await });
        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        let page = get(port, "/callback?code=4%2F0abc&state=s3cr3t&scope=x").await;
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("Compte connecté, tu peux fermer cet onglet"));
        assert_eq!(task.await.unwrap(), Ok("4/0abc".to_string()));
        // Une seule requête utile : l'écoute est fermée.
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn state_faux_refuse() {
        let (l, port) = listener().await;
        let task = tokio::spawn(async move { serve(l, "bon", Duration::from_secs(10)).await });
        assert!(get(port, "/callback?code=c&state=mauvais").await.starts_with("HTTP/1.1 400"));
        assert!(task.await.unwrap().unwrap_err().contains("state"));
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    }

    #[tokio::test]
    async fn port_pris_message_clair() {
        let (_l, port) = listener().await;
        let e = bind(port).await.unwrap_err();
        assert!(e.contains("déjà utilisé") && e.contains(&port.to_string()), "{e}");
    }

    #[tokio::test]
    async fn requete_non_get_recoit_un_404() {
        let (l, port) = listener().await;
        let task = tokio::spawn(async move { serve(l, "s", Duration::from_secs(10)).await });
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(b"POST /callback?code=c&state=s HTTP/1.1\r\n\r\n").await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 404"), "{out}");
        assert_eq!(get(port, "/callback?code=c&state=s").await.lines().next(), Some("HTTP/1.1 200 OK"));
        assert_eq!(task.await.unwrap(), Ok("c".into()));
    }

    #[tokio::test]
    async fn delai_depasse() {
        let (l, _) = listener().await;
        assert!(serve(l, "s", Duration::from_millis(50)).await.unwrap_err().contains("Délai"));
    }

    #[test]
    fn analyse_de_la_redirection() {
        assert_eq!(callback("/callback?code=c&state=s", "s"), Some(Ok("c".into())));
        assert_eq!(callback("/autre?code=c&state=s", "s"), None);
        assert!(matches!(callback("/callback?error=access_denied&state=s", "s"), Some(Err(_))));
        assert!(matches!(callback("/callback?code=c", "s"), Some(Err(_))));
        assert!(matches!(callback("/callback?state=s", "s"), Some(Err(_))));
    }
}
