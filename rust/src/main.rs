//! Everything helper, Rust edition: the same versioned JSON-lines protocol as
//! `helper/everything_helper.py`, built for a ~1 ms cold start.
//!
//! Ported providers: Hyprland, Chrome (Screen Context extension), Herdr, Kitty,
//! tmux and Neovim. AT-SPI and Ghostty run in a lazily started Python sidecar
//! (see `sidecar.rs`), only when a window that needs them is open.

mod kitty;
mod model;
mod neovim;
mod procs;
mod providers;
mod sidecar;
mod tmux;

use model::Registry;
use procs::ProcTable;
use providers::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

pub struct State {
    registry: Registry,
    metadata: HashMap<String, Value>,
    /// Public rows relayed from the Python sidecar, by provider.
    external: BTreeMap<String, Vec<Value>>,
    warnings: BTreeMap<String, Vec<String>>,
}

pub type Shared = Arc<Mutex<State>>;

fn new_context(state: &Shared) -> Context {
    Context { procs: ProcTable::read(), meta: state.lock().unwrap().metadata.clone(), ..Default::default() }
}

fn scan_provider(name: &str, ctx: &Context) -> Res<ProviderResult> {
    match name {
        "chrome" => chrome_scan(ctx),
        "herdr" => herdr_scan(ctx),
        "kitty" => kitty::scan(ctx),
        "tmux" => tmux::scan(ctx),
        "neovim" => neovim::scan(ctx),
        _ => Err("Thing provider is unavailable".into()),
    }
}

pub fn emit(message: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}

fn label(provider: &str) -> &str {
    match provider {
        "hyprland" => "Hyprland",
        "herdr" => "Herdr",
        "chrome" => "Chrome",
        "kitty" => "Kitty",
        "tmux" => "tmux",
        "neovim" => "Neovim",
        other => other,
    }
}

fn protocol_error(request_id: &str, message: &str) {
    emit(json!({"version": 1, "type": "error", "requestId": request_id, "message": message, "nonfatal": true}));
}

fn publish_partial(state: &Shared, request_id: &str, result: ProviderResult) {
    let public = {
        let mut guard = state.lock().unwrap();
        guard.warnings.insert(result.provider.to_string(), result.warnings.clone());
        guard.metadata.insert(result.provider.to_string(), result.metadata.clone());
        guard.registry.publish(result.provider, result.items)
    };
    emit(json!({"version": 1, "type": "snapshot", "requestId": request_id, "provider": result.provider, "full": false, "items": public, "warnings": result.warnings}));
}

fn publish_failure(state: &Shared, request_id: &str, provider: &str, error: &str) {
    let warning = format!("{}: {error}", label(provider));
    let public = {
        let mut guard = state.lock().unwrap();
        guard.warnings.insert(provider.to_string(), vec![warning.clone()]);
        guard.registry.publish(provider, vec![])
    };
    emit(json!({"version": 1, "type": "snapshot", "requestId": request_id, "provider": provider, "full": false, "items": public, "warnings": [warning]}));
    emit(json!({"version": 1, "type": "error", "requestId": request_id, "provider": provider, "message": warning, "nonfatal": true}));
}

fn emit_full(state: &Shared, request_id: &str) {
    let guard = state.lock().unwrap();
    let mut providers: serde_json::Map<String, Value> =
        guard.registry.provider_names().into_iter().map(|p| { let rows = guard.registry.provider_public(&p); (p, Value::Array(rows)) }).collect();
    let mut items = guard.registry.all_public();
    for (provider, rows) in &guard.external {
        items.extend(rows.iter().cloned());
        providers.insert(provider.clone(), Value::Array(rows.clone()));
    }
    let warnings: Vec<&String> = guard.warnings.values().flatten().collect();
    emit(json!({"version": 1, "type": "snapshot", "requestId": request_id, "full": true, "items": items, "providers": providers, "warnings": warnings}));
}

/// Hyprland first (it feeds every other provider), then the rest in parallel.
/// A newer scan bumps `current`, and this one stops publishing.
fn scan(state: Shared, request_id: String, options: Value, my_generation: u64, current: Arc<AtomicU64>) {
    let live = || current.load(Ordering::SeqCst) == my_generation;
    let mut ctx = new_context(&state);
    match hyprland_scan(&mut ctx) {
        Ok(result) => {
            if !live() { return; }
            ctx.meta.insert("hyprland".into(), json!({}));
            publish_partial(&state, &request_id, result);
        }
        Err(error) => {
            if live() {
                publish_failure(&state, &request_id, "hyprland", &error);
                emit_full(&state, &request_id);
            }
            return;
        }
    }
    let ctx = Arc::new(ctx);
    // AT-SPI/Ghostty live in the Python sidecar, which starts only when needed.
    let ghostty_requested = options.get("ghostty") == Some(&Value::Bool(true)) || options.get("ghosttyPalette") == Some(&Value::Bool(true));
    let sidecar_job = if sidecar::needed(&ctx, ghostty_requested) {
        let (state, request_id, options) = (state.clone(), request_id.clone(), options.clone());
        Some(std::thread::spawn(move || sidecar::scan(&state, &request_id, &options)))
    } else {
        let mut guard = state.lock().unwrap();
        for provider in sidecar::PROVIDERS {
            guard.warnings.remove(provider);
            if guard.external.remove(provider).map(|rows| !rows.is_empty()).unwrap_or(false) {
                drop(guard);
                emit(json!({"version": 1, "type": "snapshot", "requestId": request_id, "provider": provider, "full": false, "items": [], "warnings": []}));
                guard = state.lock().unwrap();
            }
        }
        None
    };
    let (tx, rx) = mpsc::channel::<(&'static str, Res<ProviderResult>)>();
    let names: [&'static str; 5] = ["chrome", "kitty", "tmux", "herdr", "neovim"];
    for name in names {
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || { let _ = tx.send((name, scan_provider(name, &ctx))); });
    }
    drop(tx);
    let mut neovim_found = false;
    for (name, outcome) in rx {
        if !live() { return; }
        match outcome {
            Ok(result) => {
                neovim_found |= name == "neovim" && !result.items.is_empty();
                publish_partial(&state, &request_id, result);
            }
            Err(error) => publish_failure(&state, &request_id, name, &error),
        }
    }
    // Neovim ran concurrently with the providers it routes through; one cheap
    // second pass sees their fresh metadata so breadcrumbs never guess a host.
    if neovim_found && live() {
        let mut second = new_context(&state);
        second.clients = ctx.clients.clone();
        second.window_ids = ctx.window_ids.clone();
        match neovim::scan(&second) {
            Ok(result) => publish_partial(&state, &request_id, result),
            Err(error) => publish_failure(&state, &request_id, "neovim", &error),
        }
    }
    if let Some(job) = sidecar_job {
        let _ = job.join();
    }
    if live() {
        emit_full(&state, &request_id);
    }
}

fn looks_stale(error: &str) -> bool {
    let text = error.to_lowercase();
    ["stale", "closed", "replaced", "reused", "no longer", "disappeared", "not found"].iter().any(|w| text.contains(w))
}

fn run_activation(state: &Shared, provider: &str, activation: &Value) -> Res<()> {
    let mut ctx = new_context(state);
    // Refresh exact managed clients without publishing a new Hyprland generation.
    let _ = hyprland_scan(&mut ctx);
    activate_provider(provider, activation, &mut ctx)
}

fn rescan(state: &Shared, provider: &str) -> Res<ProviderResult> {
    let mut ctx = new_context(state);
    let hypr = hyprland_scan(&mut ctx)?;
    if provider == "hyprland" { Ok(hypr) } else { scan_provider(provider, &ctx) }
}

fn activate(state: Shared, request_id: String, item_id: String, token: String) {
    let respond = |ok: bool, stale: bool, message: &str| {
        emit(json!({"version": 1, "type": "activation", "requestId": request_id, "itemId": item_id, "ok": ok, "stale": stale, "message": message}));
    };
    let reference = state.lock().unwrap().registry.decode(&item_id, &token);
    let reference = match reference {
        Ok(r) => r,
        Err(e) => return respond(false, true, &e),
    };
    let provider = text(&reference, "provider");
    if !["hyprland", "chrome", "herdr", "kitty", "tmux", "neovim"].contains(&provider.as_str()) {
        return respond(false, true, "Thing provider is unavailable");
    }
    let outcome = (|| -> Res<()> {
        let current = {
            let guard = state.lock().unwrap();
            guard.registry.current(&provider, &item_id).filter(|_| guard.registry.token_is_current(&reference)).map(|row| row.thing.activation.clone())
        };
        let refresh = |state: &Shared| -> Res<Value> {
            let result = rescan(state, &provider).map_err(|_| "That thing closed before it could be focused".to_string())?;
            let mut guard = state.lock().unwrap();
            guard.registry.publish(result.provider, result.items);
            guard.registry.current(&provider, &item_id).map(|row| row.thing.activation.clone()).ok_or("That thing closed before it could be focused".to_string())
        };
        let activation = match current { Some(a) => a, None => refresh(&state)? };
        match run_activation(&state, &provider, &activation) {
            Ok(()) => Ok(()),
            Err(first) if looks_stale(&first) => run_activation(&state, &provider, &refresh(&state)?),
            Err(other) => Err(other),
        }
    })();
    match outcome {
        Ok(()) => respond(true, false, ""),
        Err(error) => {
            let message = if error.trim().is_empty() { "Could not focus that thing".to_string() } else { error.clone() };
            respond(false, looks_stale(&error), &message);
        }
    }
}

fn main() {
    if !std::env::args().any(|a| a == "--json-lines") {
        eprintln!("Everything helper must be started with --json-lines");
        std::process::exit(2);
    }
    let state: Shared = Arc::new(Mutex::new(State { registry: Registry::new(), metadata: HashMap::new(), external: BTreeMap::new(), warnings: BTreeMap::new() }));
    let generation = Arc::new(AtomicU64::new(0));
    emit(json!({"version": 1, "type": "ready", "pluginVersion": "rust-0.1.0", "capabilities": {"partialSnapshots": true, "atspi": false, "ghosttyPalette": false}}));

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(&line) else {
            protocol_error("", "Malformed JSON request");
            continue;
        };
        let message = Value::Object(message);
        let request_id = text(&message, "id");
        if message.get("version") != Some(&json!(1)) || request_id.is_empty() {
            protocol_error(&request_id, "Unsupported protocol version or missing request id");
            continue;
        }
        match text(&message, "type").as_str() {
            "scan" => {
                let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
                let (state, current) = (state.clone(), generation.clone());
                let options = message.get("options").cloned().unwrap_or(json!({}));
                std::thread::spawn(move || scan(state, request_id, options, mine, current));
            }
            "activate" => {
                let (item, token) = (text(&message, "itemId"), text(&message, "token"));
                if item.is_empty() || token.is_empty() {
                    protocol_error(&request_id, "Activation requires itemId and token");
                    continue;
                }
                let owned_by_sidecar = state.lock().unwrap().external.values().flatten().any(|row| text(row, "id") == item);
                if owned_by_sidecar {
                    if !sidecar::forward(&message) {
                        emit(json!({"version": 1, "type": "activation", "requestId": request_id, "itemId": item, "ok": false, "stale": true, "message": "AT-SPI helper is not running"}));
                    }
                    continue;
                }
                let state = state.clone();
                std::thread::spawn(move || activate(state, request_id, item, token));
            }
            "shutdown" => break,
            other => protocol_error(&request_id, &format!("Unknown request type: {other}")),
        }
    }
    sidecar::shutdown();
}
