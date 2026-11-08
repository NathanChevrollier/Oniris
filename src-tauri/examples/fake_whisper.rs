//! Faux `whisper-server.exe` des tests de `modules::voice::whisper` : écoute sur `--port`, répond 200 à tout
//! (`{"text":"ok"}` sur POST) et écrit son PID dans `<exe>.pid`. Aucun réseau hors 127.0.0.1.
use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).expect("--port absent");
    let l = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
    std::fs::write(std::env::current_exe().unwrap().with_extension("pid"), std::process::id().to_string()).unwrap();
    for s in l.incoming().flatten() {
        std::thread::spawn(move || {
            let mut s = s;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            // Vide le reste de la requête (corps multipart) pour ne pas fermer sur des données non lues (RST).
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(150)));
            while s.read(&mut [0u8; 4096]).is_ok_and(|k| k > 0) {}
            let body = if buf[..n].starts_with(b"POST") { r#"{"text":"ok"}"# } else { "ok" };
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        });
    }
}
