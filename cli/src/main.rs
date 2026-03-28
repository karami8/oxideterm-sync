//! OxideTerm CLI — `oxt` command-line companion.
//!
//! Communicates with the running OxideTerm GUI via IPC
//! (Unix Domain Socket on macOS/Linux, Named Pipe on Windows).

mod connect;
mod output;
mod protocol;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(
    name = "oxt",
    about = "OxideTerm CLI companion — control OxideTerm from the command line",
    version
)]
struct Cli {
    /// Force JSON output (default: auto-detect based on terminal/pipe)
    #[arg(long, global = true)]
    json: bool,

    /// IPC timeout in milliseconds
    #[arg(long, global = true, default_value = "30000")]
    timeout: u64,

    /// Custom socket path (debugging)
    #[arg(long, global = true)]
    socket: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show OxideTerm status
    Status,

    /// List resources
    List {
        #[command(subcommand)]
        what: ListTarget,
    },

    /// Show connection health
    Health {
        /// Session ID (omit to show all)
        session_id: Option<String>,
    },

    /// Disconnect a session
    Disconnect {
        /// Session ID or name to disconnect
        target: String,
    },

    /// Manage port forwarding
    Forward {
        #[command(subcommand)]
        action: ForwardAction,
    },

    /// Query saved configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Ask AI a question (pipe context via stdin)
    Ask {
        /// Prompt text (remaining args joined)
        #[arg(trailing_var_arg = true)]
        prompt: Vec<String>,

        /// Attach terminal buffer from a session as context
        #[arg(short, long)]
        session: Option<String>,

        /// AI model to use (overrides default)
        #[arg(short, long)]
        model: Option<String>,

        /// Provider type: openai, anthropic, gemini, ollama
        #[arg(short, long)]
        provider: Option<String>,

        /// Disable streaming (wait for full response)
        #[arg(long)]
        no_stream: bool,
    },

    /// Generate code/commands with AI (code-only output)
    Exec {
        /// Prompt text (remaining args joined)
        #[arg(trailing_var_arg = true)]
        prompt: Vec<String>,

        /// Attach terminal buffer from a session as context
        #[arg(short, long)]
        session: Option<String>,

        /// AI model to use
        #[arg(short, long)]
        model: Option<String>,

        /// Provider type
        #[arg(short, long)]
        provider: Option<String>,
    },

    /// Connect to a saved connection (opens in GUI)
    Connect {
        /// Connection name, host, or ID
        target: String,
    },

    /// Open a new local terminal tab
    Open {
        /// Working directory (default: current directory)
        path: Option<String>,
    },

    /// Focus an existing session tab
    Focus {
        /// Session ID or name (omit to list available sessions)
        target: Option<String>,
    },

    /// Ping the GUI (connectivity check)
    Ping,

    /// Show version information
    Version,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ListTarget {
    /// List saved connections
    Connections,
    /// List active sessions
    Sessions,
    /// List active port forwards
    Forwards {
        /// Session ID (omit to show all sessions)
        session_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum ForwardAction {
    /// Add a port forward to a session
    Add {
        /// Forward spec: [bind_addr:]bind_port:target_host:target_port
        spec: String,

        /// Session ID or name
        #[arg(short, long)]
        session: String,

        /// Forward type: local, remote, dynamic
        #[arg(short = 't', long, default_value = "local")]
        r#type: String,

        /// Optional description
        #[arg(short, long)]
        description: Option<String>,
    },

    /// Remove a port forward
    Remove {
        /// Forward ID
        forward_id: String,

        /// Session ID or name
        #[arg(short, long)]
        session: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// List connection groups
    List,

    /// Show connection details
    Get {
        /// Connection name or ID
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let out = output::OutputMode::detect(cli.json);

    let result = run(&cli, &out);

    if let Err(e) = result {
        if out.is_json() {
            let err = serde_json::json!({ "error": e });
            eprintln!("{}", serde_json::to_string(&err).unwrap_or_default());
        } else {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

fn run(cli: &Cli, out: &output::OutputMode) -> Result<(), String> {
    // Commands that don't need IPC
    match &cli.command {
        Commands::Version => {
            out.print_version();
            return Ok(());
        }
        Commands::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "oxt",
                &mut std::io::stdout(),
            );
            return Ok(());
        }
        _ => {}
    }

    let mut conn = connect::IpcConnection::connect(cli.socket.as_deref(), cli.timeout)?;

    match &cli.command {
        Commands::Status => {
            let resp = conn.call("status", serde_json::json!({}))?;
            out.print_status(&resp);
        }
        Commands::List { what } => match what {
            ListTarget::Connections => {
                let resp = conn.call("list_saved_connections", serde_json::json!({}))?;
                out.print_connections(&resp);
            }
            ListTarget::Sessions => {
                let resp = conn.call("list_sessions", serde_json::json!({}))?;
                out.print_sessions(&resp);
            }
            ListTarget::Forwards { session_id } => {
                let params = match session_id {
                    Some(id) => serde_json::json!({ "session_id": id }),
                    None => serde_json::json!({}),
                };
                let resp = conn.call("list_forwards", params)?;
                out.print_forwards(&resp);
            }
        },
        Commands::Health { session_id } => {
            let params = match session_id {
                Some(id) => serde_json::json!({ "session_id": id }),
                None => serde_json::json!({}),
            };
            let resp = conn.call("health", params)?;
            out.print_health(&resp, session_id.is_some());
        }
        Commands::Disconnect { target } => {
            let resp = conn.call("disconnect", serde_json::json!({ "target": target }))?;
            out.print_disconnect(&resp);
        }
        Commands::Forward { action } => match action {
            ForwardAction::Add {
                spec,
                session,
                r#type,
                description,
            } => {
                let (bind_address, bind_port, target_host, target_port) =
                    parse_forward_spec(spec, r#type)?;

                // Resolve session name → ID if needed
                let session_id = resolve_session_id(&mut conn, session)?;

                let mut params = serde_json::json!({
                    "session_id": session_id,
                    "forward_type": r#type,
                    "bind_address": bind_address,
                    "bind_port": bind_port,
                    "target_host": target_host,
                    "target_port": target_port,
                });
                if let Some(desc) = description {
                    params["description"] = serde_json::json!(desc);
                }
                let resp = conn.call("create_forward", params)?;
                out.print_forward_result(&resp);
            }
            ForwardAction::Remove {
                forward_id,
                session,
            } => {
                let session_id = resolve_session_id(&mut conn, session)?;
                let resp = conn.call(
                    "delete_forward",
                    serde_json::json!({
                        "session_id": session_id,
                        "forward_id": forward_id,
                    }),
                )?;
                out.print_forward_result(&resp);
            }
        },
        Commands::Config { action } => match action {
            ConfigAction::List => {
                let resp = conn.call("config_list", serde_json::json!({}))?;
                out.print_config_list(&resp);
            }
            ConfigAction::Get { name } => {
                let resp = conn.call("config_get", serde_json::json!({ "name": name }))?;
                out.print_config_get(&resp);
            }
        },
        Commands::Ask {
            prompt,
            session,
            model,
            provider,
            no_stream,
        } => {
            let prompt_text = prompt.join(" ");
            if prompt_text.is_empty() && is_terminal_stdin() {
                return Err("No prompt provided. Usage: oxt ask \"your question\"".to_string());
            }

            // Read stdin if piped
            let context = if !is_terminal_stdin() {
                Some(read_stdin()?)
            } else {
                None
            };

            let mut params = serde_json::json!({
                "prompt": if prompt_text.is_empty() { "Analyze the following".to_string() } else { prompt_text },
                "stream": !no_stream,
            });
            if let Some(ctx) = &context {
                params["context"] = serde_json::json!(ctx);
            }
            if let Some(s) = session {
                let sid = resolve_session_id(&mut conn, s)?;
                params["session_id"] = serde_json::json!(sid);
            }
            if let Some(m) = model {
                params["model"] = serde_json::json!(m);
            }
            if let Some(p) = provider {
                params["provider"] = serde_json::json!(p);
            }

            if *no_stream {
                let resp = conn.call("ask", params)?;
                out.print_ai_response(&resp);
            } else {
                conn.call_streaming("ask", params, |text| {
                    use std::io::Write;
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                })?;
                // Ensure final newline
                println!();
            }
        }
        Commands::Exec {
            prompt,
            session,
            model,
            provider,
        } => {
            let prompt_text = prompt.join(" ");
            if prompt_text.is_empty() && is_terminal_stdin() {
                return Err("No prompt provided. Usage: oxt exec \"generate a script\"".to_string());
            }

            let context = if !is_terminal_stdin() {
                Some(read_stdin()?)
            } else {
                None
            };

            let mut params = serde_json::json!({
                "prompt": if prompt_text.is_empty() { "Generate code for the following".to_string() } else { prompt_text },
                "stream": true,
                "exec_mode": true,
            });
            if let Some(ctx) = &context {
                params["context"] = serde_json::json!(ctx);
            }
            if let Some(s) = session {
                let sid = resolve_session_id(&mut conn, s)?;
                params["session_id"] = serde_json::json!(sid);
            }
            if let Some(m) = model {
                params["model"] = serde_json::json!(m);
            }
            if let Some(p) = provider {
                params["provider"] = serde_json::json!(p);
            }

            conn.call_streaming("ask", params, |text| {
                use std::io::Write;
                print!("{text}");
                let _ = std::io::stdout().flush();
            })?;
            println!();
        }
        Commands::Connect { target } => {
            let resp = conn.call("connect", serde_json::json!({ "target": target }))?;
            out.print_connect_result(&resp);
        }
        Commands::Open { path } => {
            let dir = path.clone().unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            });
            let resp = conn.call("open_tab", serde_json::json!({ "path": dir }))?;
            out.print_json(&resp);
        }
        Commands::Focus { target } => {
            match target {
                Some(t) => {
                    let resp = conn.call("focus_tab", serde_json::json!({ "target": t }))?;
                    out.print_json(&resp);
                }
                None => {
                    // No target → list all focusable targets
                    let sessions = conn.call("list_sessions", serde_json::json!({})).unwrap_or(serde_json::json!([]));
                    let locals = conn.call("list_local_terminals", serde_json::json!({})).unwrap_or(serde_json::json!([]));
                    let ssh_items = sessions.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                    let local_items = locals.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
                    let total = ssh_items.len() + local_items.len();

                    match total {
                        0 => {
                            eprintln!("No active tabs to focus.");
                            std::process::exit(1);
                        }
                        1 => {
                            // Auto-focus the single tab
                            let (id, label) = if let Some(s) = ssh_items.first() {
                                (
                                    s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                    s.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                                )
                            } else {
                                let l = &local_items[0];
                                (
                                    l.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                    l.get("shell_name").and_then(|v| v.as_str()).unwrap_or("?"),
                                )
                            };
                            eprintln!("Auto-focusing: {label}");
                            let resp = conn.call("focus_tab", serde_json::json!({ "target": id }))?;
                            out.print_json(&resp);
                        }
                        _ => {
                            eprintln!("Multiple active tabs — specify a target:\n");
                            if !ssh_items.is_empty() {
                                eprintln!("  SSH Sessions:");
                                out.print_sessions(&sessions);
                            }
                            if !local_items.is_empty() {
                                eprintln!("\n  Local Terminals:");
                                out.print_local_terminals(&locals);
                            }
                            eprintln!("\nUsage: oxt focus <NAME-OR-ID>");
                            eprintln!("  Matches: session name/ID, shell name, or tab title");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Commands::Ping => {
            let resp = conn.call("ping", serde_json::json!({}))?;
            out.print_json(&resp);
        }
        Commands::Version | Commands::Completions { .. } => unreachable!(),
    }

    Ok(())
}

/// Parse a forward spec like `8080:localhost:80` or `0.0.0.0:8080:localhost:80`
/// Also supports IPv6 addresses in brackets: `[::1]:8080:localhost:80`
/// For dynamic forwards, spec is just `[bind_addr:]bind_port`
fn parse_forward_spec(
    spec: &str,
    fwd_type: &str,
) -> Result<(String, u16, String, u16), String> {
    // Tokenize respecting bracketed IPv6 addresses
    let tokens = tokenize_spec(spec)?;

    if fwd_type == "dynamic" {
        // Dynamic: [bind_addr:]bind_port
        return match tokens.len() {
            1 => {
                let port: u16 = tokens[0].parse().map_err(|_| format!("Invalid port: {}", tokens[0]))?;
                Ok(("127.0.0.1".to_string(), port, String::new(), 0))
            }
            2 => {
                let port: u16 = tokens[1].parse().map_err(|_| format!("Invalid port: {}", tokens[1]))?;
                Ok((tokens[0].clone(), port, String::new(), 0))
            }
            _ => Err("Dynamic forward spec: [bind_addr:]bind_port".to_string()),
        };
    }

    // local/remote: [bind_addr:]bind_port:target_host:target_port
    match tokens.len() {
        3 => {
            let bind_port: u16 = tokens[0].parse().map_err(|_| format!("Invalid bind port: {}", tokens[0]))?;
            let target_port: u16 = tokens[2].parse().map_err(|_| format!("Invalid target port: {}", tokens[2]))?;
            Ok(("127.0.0.1".to_string(), bind_port, tokens[1].clone(), target_port))
        }
        4 => {
            let bind_port: u16 = tokens[1].parse().map_err(|_| format!("Invalid bind port: {}", tokens[1]))?;
            let target_port: u16 = tokens[3].parse().map_err(|_| format!("Invalid target port: {}", tokens[3]))?;
            Ok((tokens[0].clone(), bind_port, tokens[2].clone(), target_port))
        }
        _ => Err("Forward spec: [bind_addr:]bind_port:target_host:target_port".to_string()),
    }
}

/// Tokenize a colon-separated spec, respecting `[...]` for IPv6 addresses.
/// e.g. `[::1]:8080:localhost:80` → `["::1", "8080", "localhost", "80"]`
fn tokenize_spec(spec: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut chars = spec.chars().peekable();

    while chars.peek().is_some() {
        if chars.peek() == Some(&'[') {
            // Bracketed token (IPv6)
            chars.next(); // consume '['
            let mut token = String::new();
            loop {
                match chars.next() {
                    Some(']') => break,
                    Some(c) => token.push(c),
                    None => return Err("Unclosed bracket in forward spec".to_string()),
                }
            }
            tokens.push(token);
            // Consume the following ':' separator if present
            if chars.peek() == Some(&':') {
                chars.next();
            }
        } else {
            // Regular token until next ':'
            let mut token = String::new();
            while let Some(&c) = chars.peek() {
                if c == ':' {
                    chars.next(); // consume ':'
                    break;
                }
                token.push(c);
                chars.next();
            }
            tokens.push(token);
        }
    }

    Ok(tokens)
}

/// Resolve a session target (name or ID) to a session ID.
fn resolve_session_id(conn: &mut connect::IpcConnection, target: &str) -> Result<String, String> {
    let sessions = conn.call("list_sessions", serde_json::json!({}))?;
    let items = sessions.as_array().ok_or("Invalid session list")?;

    // Try exact ID match
    if let Some(s) = items.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(target)) {
        return Ok(s["id"].as_str().unwrap().to_string());
    }
    // Try name match
    if let Some(s) = items.iter().find(|s| s.get("name").and_then(|v| v.as_str()) == Some(target)) {
        return Ok(s["id"].as_str().unwrap().to_string());
    }
    // Try partial ID match
    if let Some(s) = items.iter().find(|s| {
        s.get("id").and_then(|v| v.as_str()).map(|id| id.starts_with(target)).unwrap_or(false)
    }) {
        return Ok(s["id"].as_str().unwrap().to_string());
    }

    Err(format!("Session not found: {target}"))
}

fn is_terminal_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buf = String::new();
    // Cap stdin reading at 512KB to prevent excessively large contexts
    let max_size = 512 * 1024;
    std::io::stdin()
        .take(max_size as u64)
        .read_to_string(&mut buf)
        .map_err(|e| format!("Failed to read stdin: {e}"))?;
    Ok(buf)
}
