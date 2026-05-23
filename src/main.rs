use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use cold_agent_sdk::{Agent, AgentCallback, AgentConfig, AgentError, AgentResult};
use cold_tools::ToolResult;

// ─── Config File ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct CliConfig {
    api_key: String,
    #[serde(default = "default_base_url")]
    base_url: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_context_length")]
    context_length: u32,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default = "default_max_turns")]
    max_turns: u32,
    #[serde(default)]
    system_prompt: Option<String>,
}

fn default_base_url() -> String { "https://api.lamcold.com".to_string() }
fn default_model() -> String { "default".to_string() }
fn default_context_length() -> u32 { 128_000 }
fn default_max_turns() -> u32 { 90 }

fn config_path() -> PathBuf {
    if let Ok(dir) = std::env::var("COLD_CONFIG_DIR") {
        return PathBuf::from(dir).join("config.toml");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cold").join("config.toml")
}

fn load_config() -> CliConfig {
    let path = config_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        match toml::from_str(&content) {
            Ok(cfg) => return cfg,
            Err(e) => {
                eprintln!("  !! Failed to parse {}: {e}", path.display());
                eprintln!("  !! Run with --init to create a default config.");
                std::process::exit(1);
            }
        }
    }
    // No config file — check environment variables
    if let Ok(key) = std::env::var("COLD_API_KEY") {
        return CliConfig {
            api_key: key,
            base_url: std::env::var("COLD_BASE_URL").unwrap_or_else(|_| default_base_url()),
            model: std::env::var("COLD_MODEL").unwrap_or_else(|_| default_model()),
            context_length: std::env::var("COLD_CONTEXT_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_context_length),
            proxy: None,
            max_turns: default_max_turns(),
            system_prompt: None,
        };
    }
    eprintln!("  !! No config found at {}", path.display());
    eprintln!("  !! Set COLD_API_KEY env var, or run: cold --init");
    std::process::exit(1);
}

fn init_config() {
    let path = config_path();
    if path.exists() {
        eprintln!("  Config already exists at {}", path.display());
        eprintln!("  Edit it directly or delete it to regenerate.");
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let template = r#"# COLD Agent CLI Configuration
# Location: ~/.cold/config.toml

# Required: your API key
api_key = "your-api-key-here"

# API endpoint (without /v1, appended automatically)
base_url = "https://api.lamcold.com"

# Model identifier
model = "default"

# Context window size in tokens
context_length = 128000

# Maximum agent loop turns
max_turns = 90

# HTTP proxy (optional, also reads HTTPS_PROXY env var)
# proxy = "http://127.0.0.1:7890"

# Custom system prompt (optional)
# system_prompt = "You are a Rust expert."
"#;
    match std::fs::write(&path, template) {
        Ok(()) => {
            eprintln!("  Config created at {}", path.display());
            eprintln!("  Edit it to set your api_key, then run: cold");
        }
        Err(e) => eprintln!("  !! Failed to write config: {e}"),
    }
}

// ─── ANSI support ────────────────────────────────────────────

fn supports_ansi() -> bool {
    if std::env::var("WT_SESSION").is_ok()
        || std::env::var("ConEmuPID").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok()
        || std::env::var("TERM").is_ok()
    {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        let handle = io::stderr().as_raw_handle();
        unsafe {
            let mut mode: u32 = 0;
            if GetConsoleMode(handle, &mut mode) != 0
                && SetConsoleMode(handle, mode | 0x0004) != 0
            {
                return true;
            }
        }
    }
    false
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
}

// ─── Style ───────────────────────────────────────────────────

struct Style { ansi: bool }

impl Style {
    fn new() -> Self { Self { ansi: supports_ansi() } }
    fn paint(&self, code: &str, text: &str) -> String {
        if self.ansi { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
    }
    fn dim(&self, t: &str) -> String { self.paint("2", t) }
    fn italic_dim(&self, t: &str) -> String { self.paint("2;3", t) }
    fn red(&self, t: &str) -> String { self.paint("31", t) }
    fn green(&self, t: &str) -> String { self.paint("32", t) }
    fn yellow(&self, t: &str) -> String { self.paint("33", t) }
    fn blue(&self, t: &str) -> String { self.paint("34", t) }
    fn cyan(&self, t: &str) -> String { self.paint("36", t) }
    fn bold(&self, t: &str) -> String { self.paint("1", t) }
    fn bold_cyan(&self, t: &str) -> String { self.paint("1;36", t) }
    fn bold_blue(&self, t: &str) -> String { self.paint("1;34", t) }
}

static S: std::sync::LazyLock<Style> = std::sync::LazyLock::new(Style::new);

// ─── Utilities ───────────────────────────────────────────────

fn safe_preview(text: &str, max: usize) -> String {
    let mut len = text.len().min(max);
    while len > 0 && !text.is_char_boundary(len) { len -= 1; }
    let s = text[..len].replace('\n', " ").replace('\r', "");
    if text.len() > max { format!("{s}...") } else { s }
}

fn format_tokens(n: u32) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
    else if n >= 1_000 { format!("{:.1}K", n as f64 / 1_000.0) }
    else { n.to_string() }
}

fn resolve_proxy(cfg_proxy: &Option<String>) -> Option<String> {
    cfg_proxy.clone().or_else(||
        std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("ALL_PROXY"))
            .or_else(|_| std::env::var("all_proxy"))
            .ok()
    )
}

// ─── Callback ────────────────────────────────────────────────

struct CliCallback;

impl AgentCallback for CliCallback {
    fn on_text(&self, text: &str) {
        print!("{text}");
        let _ = io::stdout().flush();
    }
    fn on_tool_call(&self, name: &str, args: &serde_json::Value) {
        eprintln!("  {} {} {}", S.cyan(">>"), S.bold(name), S.dim(&safe_preview(&args.to_string(), 60)));
    }
    fn on_tool_result(&self, name: &str, result: &ToolResult) {
        let (icon, preview) = match result {
            ToolResult::Text(t) => ("<<", safe_preview(t, 80)),
            ToolResult::Json(_) => ("<<", "[json]".to_string()),
            ToolResult::Error { message, .. } => ("!!", format!("ERROR: {}", safe_preview(message, 60))),
            ToolResult::Empty => ("<<", "ok".to_string()),
        };
        let line = format!("  {icon} {name}: {preview}");
        if matches!(result, ToolResult::Error { .. }) {
            eprintln!("{}", S.red(&line));
        } else {
            eprintln!("{}", S.dim(&line));
        }
    }
    fn on_compress(&self, before: u32, after: u32) {
        let pct = if before > 0 { format!(" (-{}%)", ((before - after) as f64 / before as f64 * 100.0) as u32) } else { String::new() };
        eprintln!("  {} {} -> {}{pct}", S.yellow("~~"), format_tokens(before), format_tokens(after));
    }
    fn on_error(&self, error: &AgentError) { eprintln!("  {} {error}", S.red("!!")); }
    fn on_progress(&self, turn: u32, _max: u32) {
        if turn > 0 { eprintln!("{}", S.dim(&format!("  -- turn {turn} --"))); }
    }
    fn on_complete(&self, result: &AgentResult) {
        eprintln!();
        eprintln!("  {} {} turn(s) | {} tool call(s) | {} tokens",
            S.green("--"), result.turns_used, result.tools_called.len(), format_tokens(result.tokens.total_tokens));
    }
}

// ─── Banner & Config Builder ─────────────────────────────────

fn print_banner(cfg: &CliConfig, proxy: &Option<String>) {
    eprintln!();
    eprintln!("  {}  {}", S.bold_cyan("COLD"), S.dim("Agent CLI"));
    eprintln!("  {}", S.dim("Powered by LAMCLOD"));
    eprintln!();
    eprintln!("  {}  {}  {}", S.blue(&format!("model: {}", cfg.model)), S.dim("|"), S.dim(&format!("ctx: {}K", cfg.context_length / 1000)));
    if let Some(p) = proxy { eprintln!("  {}", S.dim(&format!("proxy: {p}"))); }
    eprintln!();
    eprintln!("  {}", S.italic_dim("Type a prompt to begin. /new = reset, /help = help, exit = quit."));
    eprintln!();
}

fn build_agent_config(cfg: &CliConfig, cwd: &std::path::Path, proxy: &Option<String>) -> AgentConfig {
    let mut ac = AgentConfig::new(&cfg.model, cfg.context_length, &cfg.api_key)
        .with_base_url(&cfg.base_url)
        .with_root_dir(cwd)
        .with_max_turns(cfg.max_turns)
        .with_session_dir(cwd.join(".cold/sessions"))
        .with_streaming(true);
    if let Some(p) = proxy { ac = ac.with_proxy(p); }
    if let Some(ref sp) = cfg.system_prompt { ac = ac.with_system_prompt(sp); }
    ac
}

// ─── Main ────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--init") {
        init_config();
        return;
    }
    if args.iter().any(|a| a == "--config") {
        eprintln!("  Config path: {}", config_path().display());
        return;
    }

    let cfg = load_config();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let proxy = resolve_proxy(&cfg.proxy);

    print_banner(&cfg, &proxy);

    let mut agent = match Agent::new(build_agent_config(&cfg, &cwd, &proxy)) {
        Ok(a) => a,
        Err(e) => { eprintln!("  {} Failed to initialize: {e}", S.red("!!")); std::process::exit(1); }
    };
    agent = agent.with_callback(CliCallback);

    let stdin = io::stdin();
    let mut first_turn = true;

    loop {
        let prompt_char = if first_turn { ">" } else { "..." };
        eprint!("\n  {} ", S.bold_blue(prompt_char));
        let _ = io::stderr().flush();

        let mut input = String::new();
        if stdin.lock().read_line(&mut input).is_err() || input.is_empty() { break; }
        let input = input.trim();
        if input.is_empty() { continue; }

        match input {
            "exit" | "quit" | "/exit" | "/quit" => { eprintln!("\n  {}", S.dim("Goodbye.")); break; }
            "/new" | "/reset" => {
                eprintln!("  {}", S.yellow("-- new session --"));
                agent = match Agent::new(build_agent_config(&cfg, &cwd, &proxy)) {
                    Ok(a) => a, Err(e) => { eprintln!("  {} {e}", S.red("!!")); continue; }
                };
                agent = agent.with_callback(CliCallback);
                first_turn = true;
                continue;
            }
            "/help" => {
                eprintln!();
                eprintln!("  {}", S.bold("Commands:"));
                eprintln!("    {}     start a fresh session", S.cyan("/new"));
                eprintln!("    {}    show this help", S.cyan("/help"));
                eprintln!("    {}    quit the CLI", S.cyan("exit"));
                eprintln!();
                eprintln!("  {}", S.bold("CLI flags:"));
                eprintln!("    {}  create default config at ~/.cold/config.toml", S.cyan("--init"));
                eprintln!("    {}  show config file path", S.cyan("--config"));
                eprintln!();
                continue;
            }
            _ => {}
        }

        eprintln!();
        let start = Instant::now();
        let result = if first_turn { agent.run(input).await } else { agent.continue_with(input).await };
        let elapsed = start.elapsed();

        match result {
            Ok(_) => { eprintln!("  {}", S.dim(&format!("  {:.1}s elapsed", elapsed.as_secs_f64()))); first_turn = false; }
            Err(AgentError::BudgetExhausted { turns_used, max_turns }) => {
                eprintln!("  {} budget exhausted ({turns_used}/{max_turns}). Type {} to start fresh.", S.red("!!"), S.cyan("/new"));
                first_turn = false;
            }
            Err(AgentError::Interrupted) => { eprintln!("  {}", S.yellow("-- interrupted --")); first_turn = false; }
            Err(e) => { eprintln!("  {} {e}", S.red("!!")); first_turn = false; }
        }
    }
}
