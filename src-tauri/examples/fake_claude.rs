//! Faux `claude -p` des tests de `modules::iris` : le scénario et le fichier de PID viennent du prompt (stdin),
//! `scénario:fichier_pid`. Scénarios : normal, sansoutils (init sans `tools`), sansinit (aucun init), limitesansinit (erreur de limite sans init, puis silence), limite, bash (init avec l'outil Bash), silence (init puis 10 s de rien), mcp (écrit la configuration MCP reçue au lieu du PID, puis normal).
//! Aucun réseau, aucun vrai Claude.
use std::io::{Read, Write};

fn main() {
    let mut prompt = String::new();
    std::io::stdin().read_to_string(&mut prompt).unwrap();
    let (scenario, pid_file) = prompt.trim().split_once(':').unwrap_or((prompt.trim(), ""));
    if scenario == "mcp" {
        // Écrit la configuration MCP reçue ; `args[2]` = « connecté » si le tube d'Oniris répondait.
        let args: Vec<String> = std::env::args().collect();
        let i = args.iter().position(|a| a == "--mcp-config").expect("--mcp-config absent");
        let mut cfg: serde_json::Value = serde_json::from_str(&args[i + 1]).unwrap();
        let pipe = cfg["mcpServers"]["oniris"]["args"][1].as_str().unwrap().to_string();
        let ok = std::fs::OpenOptions::new().read(true).write(true).open(&pipe).is_ok();
        cfg["mcpServers"]["oniris"]["args"].as_array_mut().unwrap().push(if ok { "connecté" } else { "injoignable" }.into());
        std::fs::write(pid_file, cfg.to_string()).unwrap();
    } else if !pid_file.is_empty() {
        std::fs::write(pid_file, std::process::id().to_string()).unwrap();
    }
    let out = |s: &str| {
        let mut o = std::io::stdout().lock();
        writeln!(o, "{s}").unwrap();
        o.flush().unwrap();
    };
    let tools = if scenario == "bash" { r#"["mcp__oniris__a","Bash"]"# } else { "[]" };
    match scenario {
        "sansoutils" => out(r#"{"type":"system","subtype":"init"}"#),
        "sansinit" | "limitesansinit" => {}
        _ => out(&format!(r#"{{"type":"system","subtype":"init","tools":{tools}}}"#)),
    }
    let delta = |t: &str| format!(r#"{{"type":"stream_event","event":{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{t}"}}}}}}"#);
    match scenario {
        "normal" | "mcp" | "sansoutils" | "sansinit" => {
            out(&delta("Bon"));
            out(&delta("jour"));
            out(r#"{"type":"result","subtype":"success","is_error":false,"result":"Bonjour"}"#);
        }
        "limite" => {
            out(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Claude AI usage limit reached|1760000000"}"#);
            std::process::exit(1);
        }
        "limitesansinit" => {
            out(r#"{"type":"result","subtype":"success","is_error":true,"result":"Claude usage limit reached"}"#);
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
        _ => std::thread::sleep(std::time::Duration::from_secs(10)),
    }
}
