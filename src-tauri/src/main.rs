#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    // Relais MCP lancé par Claude Code : jamais de fenêtre ni de base.
    if args.get(1).map(String::as_str) == Some("--mcp-relay") {
        return oniris_lib::mcp_relay::main(args.get(2).map_or("", String::as_str));
    }
    oniris_lib::run();
    std::process::ExitCode::SUCCESS
}
