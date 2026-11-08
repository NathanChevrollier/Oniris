//! Faux `piper.exe` des tests de `modules::voice::piper` : écrit son PID dans `<exe>.pid`, puis pour chaque ligne lue
//! sur stdin écrit 1000 octets de PCM (zéros) ; la ligne « LONG » écrit sans fin, 441 octets toutes les 10 ms.
use std::io::{BufRead, Write};

fn main() {
    std::fs::write(std::env::current_exe().unwrap().with_extension("pid"), std::process::id().to_string()).unwrap();
    let mut out = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line == "LONG" {
            loop {
                if out.write_all(&[0u8; 440]).and_then(|_| out.flush()).is_err() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        out.write_all(&[0u8; 1000]).unwrap();
        out.flush().unwrap();
    }
}
