//! Hyprland, Chrome (Screen Context extension) and Herdr providers.

use crate::model::{process_birth, stable_id, Thing};
use crate::procs::{basename, canonical_path, current_uid, ProcTable};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

pub type Res<T> = Result<T, String>;

#[derive(Default)]
pub struct ProviderResult {
    pub provider: &'static str,
    pub items: Vec<Thing>,
    pub warnings: Vec<String>,
    /// Routing evidence other providers read (surfaces, panes, sessions).
    pub metadata: Value,
}

#[derive(Default, Clone)]
pub struct Context {
    pub procs: ProcTable,
    /// Mapped Hyprland clients, the only routable windows.
    pub clients: Vec<Value>,
    /// hyprland address -> window item id, for parenting child rows.
    pub window_ids: std::collections::HashMap<String, String>,
    /// Latest per-provider metadata, keyed by provider name.
    pub meta: std::collections::HashMap<String, Value>,
}

impl Context {
    pub fn clients_for_pid(&self, pid: i64) -> Vec<&Value> {
        self.clients.iter().filter(|c| num(c, "pid") == pid).collect()
    }
    pub fn client_by_address(&self, address: &str) -> Option<&Value> {
        let wanted = normalized_address(address);
        self.clients.iter().find(|c| normalized_address(&text(c, "address")) == wanted)
    }
    pub fn meta_list(&self, provider: &str, key: &str) -> Vec<Value> {
        self.meta.get(provider).and_then(|m| m.get(key)).and_then(Value::as_array).cloned().unwrap_or_default()
    }
    pub fn window_item_id(&self, address: &str) -> String {
        self.window_ids.get(&normalized_address(address)).cloned().unwrap_or_default()
    }
    pub fn managed_pids(&self) -> Vec<i64> {
        self.clients.iter().map(|c| num(c, "pid")).collect()
    }
}

// ---- small helpers -------------------------------------------------------

pub fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}
pub fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
pub fn normalized_address(value: &str) -> String {
    let address = value.trim().to_lowercase();
    if !address.is_empty() && !address.starts_with("0x") { format!("0x{address}") } else { address }
}
fn valid_address(address: &str) -> bool {
    address.len() > 2 && address.starts_with("0x") && address[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Run a command with a timeout; output is collected on a helper thread so a
/// large payload cannot deadlock against the pipe buffer.
pub fn run_cmd(argv: &[&str], timeout: f64) -> Res<(i32, String, String)> {
    let (tx, rx) = mpsc::channel();
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        let out = Command::new(&owned[0]).args(&owned[1..]).stdin(std::process::Stdio::null()).output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(Duration::from_secs_f64(timeout)) {
        Ok(Ok(out)) => Ok((
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )),
        Ok(Err(error)) => Err(format!("{} failed: {error}", argv[0])),
        Err(_) => Err(format!("{} timed out", argv[0])),
    }
}

pub fn unix_json_request(path: &str, request: &Value, timeout: f64) -> Res<Value> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err("Unix socket path must be an absolute NUL-free path".into());
    }
    let stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
    let limit = Some(Duration::from_secs_f64(timeout));
    stream.set_read_timeout(limit).ok();
    stream.set_write_timeout(limit).ok();
    let mut writer = &stream;
    writer.write_all(format!("{request}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&stream).take(8 * 1024 * 1024).read_line(&mut line).map_err(|e| e.to_string())?;
    if line.is_empty() {
        return Err("socket returned an empty or oversized response".into());
    }
    serde_json::from_str::<Value>(&line)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| "socket response is not a JSON object".to_string())
}

// ---- Hyprland ------------------------------------------------------------

pub fn hypr_clients_json() -> Res<Vec<Value>> {
    // Talk to the compositor socket directly; fall back to the CLI.
    let direct = (|| -> Option<String> {
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let mut stream = UnixStream::connect(format!("{runtime}/hypr/{signature}/.socket.sock")).ok()?;
        stream.set_read_timeout(Some(Duration::from_millis(800))).ok();
        stream.write_all(b"j/clients").ok()?;
        let mut out = String::new();
        stream.read_to_string(&mut out).ok()?;
        Some(out)
    })();
    let body = match direct {
        Some(body) => body,
        None => {
            let (code, out, err) = run_cmd(&["hyprctl", "-j", "clients"], 1.0)?;
            if code != 0 {
                return Err(if err.trim().is_empty() { "hyprctl clients failed".into() } else { err.trim().into() });
            }
            out
        }
    };
    match serde_json::from_str::<Value>(&body) {
        Ok(Value::Array(rows)) => Ok(rows.into_iter().filter(Value::is_object).collect()),
        Ok(_) => Err("hyprctl client response was not a list".into()),
        Err(_) => Err("hyprctl returned invalid client JSON".into()),
    }
}

pub fn hyprland_scan(ctx: &mut Context) -> Res<ProviderResult> {
    let all = hypr_clients_json()?;
    ctx.window_ids.clear();
    ctx.clients = all.into_iter().filter(|c| c.get("mapped") != Some(&Value::Bool(false))).collect();
    let mut items = Vec::new();
    for client in &ctx.clients {
        let address = normalized_address(&text(client, "address"));
        if !valid_address(&address) {
            continue;
        }
        let pid = num(client, "pid");
        let birth = process_birth(pid);
        let class = text(client, "class");
        let initial_class = text(client, "initialClass");
        let initial_title = text(client, "initialTitle");
        let app_class = [&class, &initial_class].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or("Application".into());
        let title = [text(client, "title"), initial_title.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or(app_class.clone());
        let workspace = client.get("workspace").filter(|w| w.is_object());
        let workspace_name = workspace
            .map(|w| [text(w, "name"), text(w, "id")].into_iter().find(|s| !s.is_empty() && s != "0").unwrap_or_default())
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown".into());
        let hidden = client.get("hidden") == Some(&Value::Bool(true));
        let mut badges = vec!["Window".to_string(), format!("Workspace {workspace_name}")];
        if hidden { badges.push("Hidden".into()); }
        if client.get("grouped").and_then(Value::as_array).map(|g| !g.is_empty()).unwrap_or(false) {
            badges.push("Group".into());
        }
        if workspace_name.starts_with("special:") { badges.push("Scratchpad".into()); }
        let focus_history = num(client, "focusHistoryID");
        let mut thing = Thing::new(stable_id("hyprland", &[&address, &pid.to_string(), &birth]), "window", "Hyprland", &title);
        thing.icon_hints = [text(client, "icon"), class.clone(), initial_class.clone(), initial_title.clone()].into();
        thing.context = format!("{app_class} · workspace {workspace_name}");
        thing.search_terms = vec![app_class, initial_class, initial_title, workspace_name];
        thing.badges = badges;
        thing.active = focus_history == 0 && !hidden;
        thing.recency = (4000 - focus_history * 100).max(-4000) as f64;
        thing.activation = json!({"address": address, "pid": pid, "birth": birth});
        ctx.window_ids.insert(address.clone(), thing.id.clone());
        items.push(thing);
    }
    Ok(ProviderResult { provider: "hyprland", items, ..Default::default() })
}

pub fn focus_address(address: &str) -> Res<()> {
    let target = normalized_address(address);
    if !valid_address(&target) {
        return Err("refusing an invalid Hyprland address".into());
    }
    let expression = format!("hl.dsp.focus({{ window = {} }})", Value::String(format!("address:{target}")));
    let (code, out, err) = run_cmd(&["hyprctl", "dispatch", &expression], 1.0)?;
    if code != 0 || !matches!(out.trim(), "" | "ok") {
        let detail = if !err.trim().is_empty() { err.trim() } else { out.trim() };
        return Err(if detail.is_empty() { "Hyprland focus failed".into() } else { detail.into() });
    }
    Ok(())
}

pub fn hyprland_activate(activation: &Value, ctx: &mut Context) -> Res<()> {
    let address = normalized_address(&text(activation, "address"));
    if ctx.client_by_address(&address).is_none() {
        hyprland_scan(ctx)?;
    }
    let client = ctx.client_by_address(&address).ok_or("window is no longer managed")?;
    let pid = num(client, "pid");
    if pid != num(activation, "pid") || process_birth(pid) != text(activation, "birth") {
        return Err("window address was reused by another process".into());
    }
    focus_address(&address)
}

pub fn launch_terminal_and_focus(argv: &[&str]) -> Res<()> {
    let before: Vec<String> = hypr_clients_json().unwrap_or_default().iter().map(|c| normalized_address(&text(c, "address"))).collect();
    Command::new(argv[0]).args(&argv[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|e| e.to_string())?;
    for _ in 0..64 {
        if let Ok(clients) = hypr_clients_json() {
            let fresh: Vec<&Value> = clients
                .iter()
                .filter(|c| c.get("mapped") != Some(&Value::Bool(false)))
                .filter(|c| !before.contains(&normalized_address(&text(c, "address"))))
                .filter(|c| {
                    let class = [text(c, "class"), text(c, "initialClass")].into_iter().find(|s| !s.is_empty()).unwrap_or_default().to_lowercase();
                    ["kitty", "ghostty", "foot", "alacritty"].iter().any(|t| class.contains(t))
                })
                .collect();
            if fresh.len() == 1 {
                return focus_address(&text(fresh[0], "address"));
            }
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    Err("fresh terminal client could not be identified".into())
}

// ---- Chrome (Screen Context extension) -----------------------------------

pub fn chrome_extension_available() -> bool {
    std::fs::metadata(chrome_socket()).map(|m| std::os::unix::fs::FileTypeExt::is_socket(&m.file_type())).unwrap_or(false)
}

fn chrome_socket() -> String {
    std::env::var("SCREEN_CONTEXT_SOCK").unwrap_or_else(|_| {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or("/tmp".into());
        format!("{runtime}/screen-context/host.sock")
    })
}

fn is_chrome_client(client: &Value) -> bool {
    let class = text(client, "class").to_lowercase();
    class.starts_with("google-chrome") || class.starts_with("chromium")
}

fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_lowercase()
}

pub fn chrome_scan(ctx: &Context) -> Res<ProviderResult> {
    let mut result = ProviderResult { provider: "chrome", ..Default::default() };
    if !ctx.clients.iter().any(is_chrome_client) {
        return Ok(result);
    }
    let Ok(response) = unix_json_request(&chrome_socket(), &json!({"op": "tabs"}), 2.0) else { return Ok(result) };
    if response.get("ok") != Some(&Value::Bool(true)) {
        let detail = response.get("error").and_then(Value::as_str).unwrap_or("failed");
        result.warnings.push(format!("Chrome tabs: {detail}"));
        return Ok(result);
    }
    for tab in response.get("result").and_then(Value::as_array).into_iter().flatten() {
        let Some(tab_id) = tab.get("id").and_then(Value::as_i64) else { continue };
        if tab.get("windowType").and_then(Value::as_str).unwrap_or("normal") != "normal" {
            continue;
        }
        let url = text(tab, "url");
        let page_title = text(tab, "title");
        let title = if !page_title.is_empty() { page_title.clone() } else if !url.is_empty() { url.clone() } else { "New Tab".into() };
        let mut thing = Thing::new(stable_id("chrome", &[&tab_id.to_string()]), "browser-tab", "Chrome", &title);
        thing.icon_hints = vec!["google-chrome".into()];
        thing.context = url_host(&url);
        thing.search_terms = vec!["chrome".into(), url.clone()];
        thing.badges = vec!["Tab".into()];
        if tab.get("pinned") == Some(&Value::Bool(true)) { thing.badges.push("Pinned".into()); }
        if tab.get("audible") == Some(&Value::Bool(true)) { thing.badges.push("Audible".into()); }
        thing.active = tab.get("active") == Some(&Value::Bool(true));
        thing.activation = json!({"tab_id": tab_id, "title": page_title, "url": url});
        result.items.push(thing);
    }
    Ok(result)
}

pub fn chrome_activate(activation: &Value) -> Res<()> {
    let tab_id = activation.get("tab_id").and_then(Value::as_i64).ok_or("Chrome tab is no longer available")?;
    let response = unix_json_request(&chrome_socket(), &json!({"op": "activate_tab", "tabId": tab_id}), 2.0)
        .map_err(|_| "Chrome extension is not reachable".to_string())?;
    if response.get("ok") != Some(&Value::Bool(true)) {
        return Err("That tab is no longer available".into());
    }
    let wanted = response.get("result").map(|r| text(r, "title")).filter(|t| !t.is_empty()).unwrap_or(text(activation, "title"));
    for _ in 0..8 {
        let clients: Vec<Value> = hypr_clients_json()?.into_iter().filter(|c| is_chrome_client(c) && c.get("mapped") == Some(&Value::Bool(true))).collect();
        let matches: Vec<&Value> = clients.iter().filter(|c| !wanted.is_empty() && text(c, "title").starts_with(&wanted)).collect();
        if matches.len() == 1 || (!matches.is_empty() && clients.len() > 1) {
            return focus_address(&text(matches[0], "address"));
        }
        if wanted.is_empty() && clients.len() == 1 {
            return focus_address(&text(&clients[0], "address"));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("Could not find the Chrome window for that tab".into())
}

// ---- Routing -------------------------------------------------------------

/// Resolve one process to a proven terminal surface or managed window.
/// Kitty/Ghostty surfaces win over their outer client; with several surfaces
/// only an exact title plus cwd match counts; otherwise the caller must open
/// a fresh client rather than guess.
pub fn route_for_process(ctx: &Context, pid: i64, title: &str, cwd: &str) -> Option<Value> {
    let wanted_cwd = canonical_path(cwd);
    let ancestors = ctx.procs.ancestors(pid);
    let ancestor_pids: Vec<i64> = ancestors.iter().map(|p| p.pid).collect();

    for provider in ["kitty", "ghostty"] {
        let surfaces: Vec<Value> = ctx.meta_list(provider, "surfaces").into_iter().filter(|s| ancestor_pids.contains(&num(s, "pid"))).collect();
        if surfaces.is_empty() {
            continue;
        }
        let exact: Vec<&Value> = if surfaces.len() == 1 {
            surfaces.iter().collect()
        } else {
            surfaces.iter().filter(|s| !title.is_empty() && text(s, "title") == title && !wanted_cwd.is_empty() && canonical_path(&text(s, "cwd")) == wanted_cwd).collect()
        };
        if exact.len() == 1 {
            let s = exact[0];
            return Some(json!({"provider": provider, "activation": s.get("activation").cloned().unwrap_or(json!({})), "item_id": text(s, "item_id")}));
        }
        return None;
    }

    // A process inside an internal terminal with no current metadata would be
    // guessed onto the outer window. Ghostty window routing stays exact when
    // pane discovery is off and its PID owns exactly one managed window.
    let ghostty_has_surfaces = !ctx.meta_list("ghostty", "surfaces").is_empty();
    let is_internal = |p: &&crate::procs::Proc| {
        let exe = p.argv.first().map(|a| basename(a).to_lowercase()).unwrap_or_default();
        let comm = p.comm.to_lowercase();
        comm == "kitty" || exe == "kitty" || (ghostty_has_surfaces && (comm == "ghostty" || exe == "ghostty"))
    };
    if ancestors.iter().any(|p| is_internal(&p)) {
        return None;
    }

    let ancestor_pid = ctx.procs.ancestor_pid_in(pid, &ctx.managed_pids())?;
    let clients = ctx.clients_for_pid(ancestor_pid);
    if clients.len() != 1 {
        return None;
    }
    let address = normalized_address(&text(clients[0], "address"));
    Some(json!({
        "provider": "hyprland",
        "activation": {"address": address, "pid": ancestor_pid, "birth": process_birth(ancestor_pid)},
        "item_id": ctx.window_item_id(&address),
    }))
}

/// Activate a routed or published target by provider name.
pub fn activate_provider(provider: &str, activation: &Value, ctx: &mut Context) -> Res<()> {
    match provider {
        "hyprland" => hyprland_activate(activation, ctx),
        "chrome" => chrome_activate(activation),
        "herdr" => herdr_activate(activation, ctx),
        "kitty" => crate::kitty::activate(activation, ctx),
        "tmux" => crate::tmux::activate(activation, ctx),
        "neovim" => crate::neovim::activate(activation, ctx),
        _ => Err("Thing provider is unavailable".into()),
    }
}

// ---- Herdr ---------------------------------------------------------------

fn native_id(value: Option<&Value>) -> Option<String> {
    let s = value?.as_str()?;
    (!s.trim().is_empty() && !s.contains(['\0', '\n', '\r'])).then(|| s.to_string())
}

fn rows(snapshot: &Value, key: &str, id_key: &str, session: &str, warnings: &mut Vec<String>) -> Vec<Value> {
    let Some(all) = snapshot.get(key).and_then(Value::as_array).or_else(|| snapshot.get(key).is_none().then_some(&EMPTY)) else {
        warnings.push(format!("Herdr session {session}: {key} collection is not an array"));
        return vec![];
    };
    let valid: Vec<&Value> = all.iter().filter(|r| r.is_object() && native_id(r.get(id_key)).is_some()).collect();
    let unique: Vec<Value> = valid
        .iter()
        .filter(|r| valid.iter().filter(|o| o.get(id_key) == r.get(id_key)).count() == 1)
        .map(|r| (*r).clone())
        .collect();
    if unique.len() != all.len() {
        warnings.push(format!("Herdr session {session}: {key} contains invalid or ambiguous native IDs"));
    }
    unique
}
static EMPTY: Vec<Value> = Vec::new();

fn herdr_session_processes<'a>(ctx: &'a Context, session: &str) -> Vec<&'a crate::procs::Proc> {
    ctx.procs
        .named("herdr")
        .into_iter()
        .filter(|p| !p.argv.iter().any(|a| a == "server"))
        .filter(|p| {
            let mut name = "default";
            for (i, arg) in p.argv.iter().enumerate().take(p.argv.len().saturating_sub(1)) {
                if arg == "--session" || (arg == "attach" && i > 0 && p.argv[i - 1] == "session") {
                    name = &p.argv[i + 1];
                }
            }
            name == session
        })
        .collect()
}

fn herdr_session_hosts(ctx: &Context, session: &str) -> Vec<Value> {
    let managed = ctx.managed_pids();
    let mut hosts: Vec<Value> = Vec::new();
    for process in herdr_session_processes(ctx, session) {
        let ancestor = ctx.procs.ancestor_pid_in(process.pid, &managed).unwrap_or(0);
        let clients = ctx.clients_for_pid(ancestor);
        if clients.len() != 1 {
            continue;
        }
        let host = json!({"address": normalized_address(&text(clients[0], "address")), "pid": ancestor.to_string()});
        if !hosts.contains(&host) { hosts.push(host); }
    }
    hosts
}

fn server_pid_for_socket(socket: &str, ctx: &Context) -> i64 {
    let Ok(table) = std::fs::read_to_string("/proc/net/unix") else { return 0 };
    let inode = table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.len() >= 8 && fields[7] == socket).then(|| fields[6].to_string())
    });
    let Some(inode) = inode else { return 0 };
    let marker = format!("socket:[{inode}]");
    for process in ctx.procs.named("herdr") {
        if !process.argv.iter().any(|a| a == "server") { continue; }
        let Ok(dir) = std::fs::read_dir(format!("/proc/{}/fd", process.pid)) else { continue };
        if dir.flatten().any(|e| std::fs::read_link(e.path()).map(|l| l.to_string_lossy() == marker).unwrap_or(false)) {
            return process.pid;
        }
    }
    0
}

pub fn validate_socket(path: &str) -> Option<std::fs::Metadata> {
    if !path.starts_with('/') { return None; }
    let meta = std::fs::symlink_metadata(path).ok()?;
    (meta.uid() == current_uid() && meta.file_type().is_socket_like()).then_some(meta)
}
trait SocketLike { fn is_socket_like(&self) -> bool; }
impl SocketLike for std::fs::FileType {
    fn is_socket_like(&self) -> bool { std::os::unix::fs::FileTypeExt::is_socket(self) }
}

fn herdr_activation(socket: &str, session: &str, server_pid: i64, birth: &str, kind: &str, target: &str, hosts: &[Value]) -> Value {
    let (dev, ino) = std::fs::symlink_metadata(socket).map(|m| (m.dev(), m.ino())).unwrap_or((0, 0));
    json!({"socket": socket, "session": session, "server_pid": server_pid, "birth": birth, "socket_dev": dev, "socket_ino": ino, "target_kind": kind, "target_id": target, "hosts": hosts})
}

fn tl(s: &str) -> String {
    // Python str.title() for the simple lowercase words herdr emits.
    let mut out = String::new();
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start { out.extend(c.to_uppercase()); } else { out.extend(c.to_lowercase()); }
            start = false;
        } else { out.push(c); start = true; }
    }
    out
}

pub fn herdr_scan(ctx: &Context) -> Res<ProviderResult> {
    let (code, out, err) = run_cmd(&["herdr", "session", "list", "--json"], 1.2)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Herdr session discovery failed".into() } else { err.trim().into() });
    }
    let listing: Value = serde_json::from_str(&out).map_err(|_| "Herdr session list returned invalid JSON")?;
    let mut result = ProviderResult { provider: "herdr", ..Default::default() };
    let (mut panes_meta, mut sessions_meta): (Vec<Value>, Vec<Value>) = (vec![], vec![]);
    for session in listing.get("sessions").and_then(Value::as_array).into_iter().flatten() {
        if session.get("running") != Some(&Value::Bool(true)) { continue; }
        let name = Some(text(session, "name")).filter(|n| !n.is_empty()).unwrap_or("default".into());
        let socket = text(session, "socket_path");
        let Some(socket_info) = validate_socket(&socket) else {
            result.warnings.push(format!("Herdr session {name} has no safe local socket"));
            continue;
        };
        let server_pid = server_pid_for_socket(&socket, ctx);
        let birth = if server_pid != 0 { process_birth(server_pid) } else { format!("socket-{}", socket_info.ino()) };
        let response = match unix_json_request(&socket, &json!({"id": "everything:snapshot", "method": "session.snapshot", "params": {}}), 1.1) {
            Ok(r) => r,
            Err(e) => { result.warnings.push(format!("Herdr session {name}: {e}")); continue; }
        };
        let snapshot = match herdr_snapshot(&response) {
            Ok(s) => s,
            Err(e) => { result.warnings.push(format!("Herdr session {name}: {e}")); continue; }
        };
        let workspaces = rows(&snapshot, "workspaces", "workspace_id", &name, &mut result.warnings);
        let tabs = rows(&snapshot, "tabs", "tab_id", &name, &mut result.warnings);
        let panes = rows(&snapshot, "panes", "pane_id", &name, &mut result.warnings);
        let agents = rows(&snapshot, "agents", "pane_id", &name, &mut result.warnings);

        let identity = [socket.clone(), server_pid.to_string(), birth.clone(), socket_info.dev().to_string(), socket_info.ino().to_string()];
        let id = |parts: &[&str]| {
            let mut all: Vec<&str> = identity.iter().map(String::as_str).collect();
            all.extend_from_slice(parts);
            stable_id("herdr", &all)
        };
        let hosts = herdr_session_hosts(ctx, &name);
        let act = |kind: &str, target: &str| herdr_activation(&socket, &name, server_pid, &birth, kind, target, &hosts);
        let session_id = id(&["session", &name]);
        let (fw, ft, fp) = (text(&snapshot, "focused_workspace_id"), text(&snapshot, "focused_tab_id"), text(&snapshot, "focused_pane_id"));
        let version = text(&snapshot, "version");

        let mut thing = Thing::new(session_id.clone(), "herdr-session", "Herdr", &name);
        thing.context = format!("Herdr {version} · local session").trim_matches([' ', '·']).to_string();
        thing.search_terms = vec!["herdr".into(), socket.clone()];
        thing.badges = vec!["Session".into(), if hosts.is_empty() { "Detached".into() } else { "Attached".into() }];
        thing.active = !hosts.is_empty() && !fw.is_empty();
        thing.recency = if hosts.is_empty() { 0.0 } else { 3000.0 };
        thing.activation = act("session", "");
        result.items.push(thing);

        let mut workspace_ids = std::collections::HashMap::new();
        for w in &workspaces {
            let nid = text(w, "workspace_id");
            let item_id = id(&["workspace", &nid]);
            workspace_ids.insert(nid.clone(), item_id.clone());
            let status = Some(text(w, "agent_status")).filter(|s| !s.is_empty()).unwrap_or("unknown".into());
            let label = Some(text(w, "label")).filter(|s| !s.is_empty()).unwrap_or(nid.clone());
            let mut t = Thing::new(item_id, "herdr-workspace", "Herdr", &label);
            t.context = format!("{} tabs · {} panes", num(w, "tab_count"), num(w, "pane_count"));
            t.search_terms = vec!["herdr".into(), name.clone(), nid.clone(), status.clone()];
            t.parent_id = session_id.clone();
            t.badges = vec!["Workspace".into(), tl(&status)];
            t.active = nid == fw;
            t.recency = if nid == fw { 3400.0 } else { 0.0 };
            t.activation = act("workspace", &nid);
            result.items.push(t);
        }
        let mut tab_ids = std::collections::HashMap::new();
        for tab in &tabs {
            let nid = text(tab, "tab_id");
            let workspace_id = text(tab, "workspace_id");
            let item_id = id(&["tab", &nid]);
            tab_ids.insert(nid.clone(), item_id.clone());
            let status = Some(text(tab, "agent_status")).filter(|s| !s.is_empty()).unwrap_or("unknown".into());
            let label = Some(text(tab, "label")).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Tab {}", Some(text(tab, "number")).filter(|s| !s.is_empty()).unwrap_or(nid.clone())));
            let mut t = Thing::new(item_id, "herdr-tab", "Herdr", &label);
            t.context = format!("{} panes · {name}", num(tab, "pane_count"));
            t.search_terms = vec!["herdr".into(), workspace_id.clone(), nid.clone(), status.clone()];
            t.parent_id = workspace_ids.get(&workspace_id).cloned().unwrap_or(session_id.clone());
            t.badges = vec!["Tab".into(), tl(&status)];
            t.active = nid == ft;
            t.recency = if nid == ft { 3600.0 } else { 0.0 };
            t.activation = act("tab", &nid);
            result.items.push(t);
        }
        let mut pane_ids = std::collections::HashMap::new();
        for pane in &panes {
            let nid = text(pane, "pane_id");
            let tab_id = text(pane, "tab_id");
            let item_id = id(&["pane", &nid]);
            pane_ids.insert(nid.clone(), item_id.clone());
            let cwd = canonical_path(&[text(pane, "foreground_cwd"), text(pane, "cwd")].into_iter().find(|s| !s.is_empty()).unwrap_or_default());
            let title = [text(pane, "terminal_title_stripped"), text(pane, "terminal_title"), basename(&cwd).to_string(), nid.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            let status = Some(text(pane, "agent_status")).filter(|s| !s.is_empty()).unwrap_or("unknown".into());
            let mut t = Thing::new(item_id, "herdr-pane", "Herdr", &title);
            t.context = if cwd.is_empty() { name.clone() } else { cwd.clone() };
            t.search_terms = vec!["herdr".into(), nid.clone(), tab_id.clone(), status.clone()];
            t.parent_id = tab_ids.get(&tab_id).cloned().unwrap_or(session_id.clone());
            t.badges = vec!["Pane".into(), tl(&status)];
            t.active = nid == fp;
            t.recency = if nid == fp { 3800.0 } else { 0.0 };
            t.activation = act("pane", &nid);
            panes_meta.push(json!({"item_id": t.id, "session": name, "socket": socket, "server_pid": server_pid, "birth": birth, "pane_id": nid, "cwd": cwd, "title": t.title, "activation": t.activation}));
            result.items.push(t);
        }
        for agent in &agents {
            let pane_id = text(agent, "pane_id");
            let reference = agent.get("agent_session").filter(|r| r.is_object());
            let identity_value = reference.map(|r| text(r, "value")).filter(|s| !s.is_empty()).unwrap_or(pane_id.clone());
            let agent_name = Some(text(agent, "agent")).filter(|s| !s.is_empty()).unwrap_or("Agent".into());
            let status = Some(text(agent, "agent_status")).filter(|s| !s.is_empty()).unwrap_or("unknown".into());
            let cwd = canonical_path(&[text(agent, "foreground_cwd"), text(agent, "cwd")].into_iter().find(|s| !s.is_empty()).unwrap_or_default());
            let terminal_title = [text(agent, "terminal_title_stripped"), text(agent, "terminal_title")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            let item_id = id(&["agent", &agent_name, &identity_value, &pane_id]);
            let mut t = Thing::new(item_id, "herdr-agent", "Herdr", &if terminal_title.is_empty() { tl(&agent_name) } else { terminal_title });
            t.context = if cwd.is_empty() { name.clone() } else { cwd };
            t.search_terms = vec!["herdr".into(), agent_name.clone(), identity_value, pane_id.clone(), status.clone()];
            t.parent_id = pane_ids.get(&pane_id).cloned().unwrap_or(session_id.clone());
            t.badges = vec![tl(&agent_name), tl(&status)];
            t.active = pane_id == fp && agent.get("focused") == Some(&Value::Bool(true));
            t.recency = if pane_id == fp { 4000.0 } else { agent.get("state_change_seq").and_then(Value::as_i64).map(|v| (v % 1000) as f64).unwrap_or(0.0) };
            t.activation = act("pane", &pane_id);
            result.items.push(t);
        }
        sessions_meta.push(json!({"name": name, "socket": socket, "server_pid": server_pid, "birth": birth, "hosts": hosts}));
    }
    result.metadata = json!({"panes": panes_meta, "sessions": sessions_meta});
    Ok(result)
}

fn herdr_snapshot(response: &Value) -> Res<Value> {
    if let Some(error) = response.get("error").filter(|e| e.is_object()) {
        return Err(Some(text(error, "message")).filter(|m| !m.is_empty()).unwrap_or("Herdr request failed".into()));
    }
    let result = response.get("result").filter(|r| r.is_object() && text(r, "type") == "session_snapshot").ok_or("Herdr did not return a session snapshot")?;
    result.get("snapshot").filter(|s| s.is_object()).cloned().ok_or_else(|| "Herdr snapshot payload is missing".to_string())
}

pub fn herdr_activate(activation: &Value, ctx: &mut Context) -> Res<()> {
    let socket = text(activation, "socket");
    let info = validate_socket(&socket).ok_or("Herdr session socket closed")?;
    if activation.get("socket_dev").and_then(Value::as_u64) != Some(info.dev()) || activation.get("socket_ino").and_then(Value::as_u64) != Some(info.ino()) {
        return Err("Herdr session socket was replaced".into());
    }
    let server_pid = num(activation, "server_pid");
    if server_pid != 0 && process_birth(server_pid) != text(activation, "birth") {
        return Err("Herdr server was replaced".into());
    }
    let kind = text(activation, "target_kind");
    if !["session", "workspace", "tab", "pane"].contains(&kind.as_str()) {
        return Err("Unknown Herdr activation target".into());
    }
    let target = native_id(activation.get("target_id"));
    if kind != "session" && target.is_none() {
        return Err("Herdr target has no exact native ID".into());
    }
    if let Some(method) = match kind.as_str() { "workspace" => Some("workspace.focus"), "tab" => Some("tab.focus"), "pane" => Some("pane.focus"), _ => None } {
        let response = unix_json_request(&socket, &json!({"id": "everything:focus", "method": method, "params": {format!("{kind}_id"): target}}), 1.0)?;
        if let Some(error) = response.get("error").filter(|e| e.is_object()) {
            return Err(Some(text(error, "message")).filter(|m| !m.is_empty()).unwrap_or("Herdr target closed".into()));
        }
        if !response.get("result").map(Value::is_object).unwrap_or(false) {
            return Err("Herdr focus did not return a result".into());
        }
    }
    let session = Some(text(activation, "session")).filter(|s| !s.is_empty()).unwrap_or("default".into());
    let cwds: Vec<(i64, String)> = herdr_session_processes(ctx, &session).iter().map(|p| (p.pid, crate::procs::proc_cwd(p.pid))).collect();
    let routes: Vec<Value> = cwds.iter().filter_map(|(pid, cwd)| route_for_process(ctx, *pid, "", cwd)).collect();
    if routes.len() == 1 {
        let route = &routes[0];
        let provider = text(route, "provider");
        let activation = route.get("activation").cloned().unwrap_or(json!({}));
        return activate_provider(&provider, &activation, ctx);
    }
    if activation.get("allow_new_client") == Some(&Value::Bool(false)) {
        return Ok(());
    }
    launch_terminal_and_focus(&["omarchy-launch-terminal", "herdr", "session", "attach", &session])
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_hosts_drop_credentials_ports_and_case() {
        assert_eq!(url_host("https://User@News.YCombinator.com:8443/item?id=1"), "news.ycombinator.com");
        assert_eq!(url_host("chrome://extensions"), "extensions");
        assert_eq!(url_host("not a url"), "");
    }

    #[test]
    fn addresses_are_normalized_and_validated() {
        assert_eq!(normalized_address(" ABC123 "), "0xabc123");
        assert!(valid_address("0xabc123"));
        assert!(!valid_address("0xzz"));
        assert!(!valid_address("0x"));
    }
}
