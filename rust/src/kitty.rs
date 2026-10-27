//! Kitty tabs and panes through `kitten @ ls` on per-instance sockets.

use crate::model::{process_birth, stable_id, Thing};
use crate::procs::{canonical_path, current_uid};
use crate::providers::*;
use serde_json::{json, Value};
use std::os::unix::fs::{FileTypeExt, MetadataExt};

fn sockets(ctx: &Context) -> Vec<(String, i64, String, u64, u64)> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", current_uid()));
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&runtime) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix("omarchy-kitty-").and_then(|n| n.parse::<i64>().ok()) else { continue };
        let Some(process) = ctx.procs.get(pid) else { continue };
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
        let blob = format!("{} {}", process.comm, process.argv.join(" ")).to_lowercase();
        if meta.uid() != current_uid() || !meta.file_type().is_socket() || !blob.contains("kitty") {
            continue;
        }
        out.push((entry.path().to_string_lossy().into_owned(), pid, process.start_time.clone(), meta.dev(), meta.ino()));
    }
    out.sort();
    out
}

fn match_host<'a>(ctx: &'a Context, pid: i64, tabs: &[Value]) -> Option<&'a Value> {
    let clients = ctx.clients_for_pid(pid);
    if clients.len() == 1 {
        return Some(clients[0]);
    }
    let focused = tabs.iter().find(|t| t.get("is_focused") == Some(&Value::Bool(true))).or(tabs.first());
    let title = focused.map(|t| text(t, "title")).unwrap_or_default();
    let matches: Vec<&Value> = clients.into_iter().filter(|c| text(c, "title") == title).collect();
    (matches.len() == 1).then(|| matches[0])
}

fn tab_context(windows: &[&Value]) -> String {
    let paths: Vec<String> = windows.iter().map(|w| canonical_path(&text(w, "cwd"))).filter(|p| !p.is_empty()).collect();
    match paths.first() {
        Some(first) if paths.iter().all(|p| p == first) => first.clone(),
        _ => "Kitty".into(),
    }
}

pub fn scan(ctx: &Context) -> Res<ProviderResult> {
    let mut result = ProviderResult { provider: "kitty", ..Default::default() };
    let mut surfaces = Vec::new();
    for (socket, pid, birth, dev, ino) in sockets(ctx) {
        let short = socket.rsplit('/').next().unwrap_or(&socket).to_string();
        let target = format!("unix:{socket}");
        let Ok((code, out, _)) = run_cmd(&["kitten", "@", "--to", &target, "ls"], 1.2) else {
            result.warnings.push(format!("Kitty socket {short} is stale"));
            continue;
        };
        if code != 0 {
            result.warnings.push(format!("Kitty socket {short} is stale"));
            continue;
        }
        let Ok(Value::Array(os_windows)) = serde_json::from_str::<Value>(&out) else {
            result.warnings.push(format!("Kitty socket {short} returned invalid data"));
            continue;
        };
        for os_window in os_windows.iter().filter(|w| w.is_object()) {
            let os_id = num(os_window, "id");
            let tabs: Vec<Value> = os_window.get("tabs").and_then(Value::as_array).into_iter().flatten().filter(|t| t.is_object()).cloned().collect();
            let host = match_host(ctx, pid, &tabs);
            let host_address = host.map(|h| normalized_address(&text(h, "address"))).unwrap_or_default();
            let parent_window = ctx.window_item_id(&host_address);
            let focused_os = os_window.get("is_focused") == Some(&Value::Bool(true));
            for tab in &tabs {
                let tab_id = num(tab, "id");
                if tab_id <= 0 { continue; }
                let windows: Vec<&Value> = tab.get("windows").and_then(Value::as_array).into_iter().flatten().filter(|w| w.is_object()).collect();
                let tab_title = Some(text(tab, "title")).filter(|t| !t.is_empty()).unwrap_or("Kitty tab".into());
                let tab_item = stable_id("kitty", &[&socket, &dev.to_string(), &ino.to_string(), &pid.to_string(), &birth, &os_id.to_string(), "tab", &tab_id.to_string()]);
                let tab_active = focused_os && tab.get("is_focused") == Some(&Value::Bool(true));
                let context = tab_context(&windows);
                let mut t = Thing::new(tab_item.clone(), "terminal-tab", "Kitty", &tab_title);
                t.context = context.clone();
                t.search_terms = vec!["kitty".into(), context];
                t.parent_id = parent_window.clone();
                t.badges = vec!["Tab".into()];
                t.active = tab_active;
                t.recency = if tab_active { 3500.0 } else { 0.0 };
                t.activation = json!({"socket": socket, "socket_dev": dev, "socket_ino": ino, "pid": pid, "birth": birth, "os_window_id": os_id, "tab_id": tab_id, "address": host_address, "target": "tab"});
                result.items.push(t);
                for window in windows {
                    let window_id = num(window, "id");
                    if window_id <= 0 { continue; }
                    let cwd = canonical_path(&text(window, "cwd"));
                    let title = [text(window, "title"), tab_title.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or("Kitty pane".into());
                    let active = tab_active && window.get("is_focused") == Some(&Value::Bool(true));
                    let pane_item = stable_id("kitty", &[&socket, &dev.to_string(), &ino.to_string(), &pid.to_string(), &birth, &os_id.to_string(), "pane", &window_id.to_string()]);
                    let activation = json!({"socket": socket, "socket_dev": dev, "socket_ino": ino, "pid": pid, "birth": birth, "os_window_id": os_id, "window_id": window_id, "address": host_address, "target": "window"});
                    let mut p = Thing::new(pane_item.clone(), "terminal-pane", "Kitty", &title);
                    p.context = if cwd.is_empty() { tab_title.clone() } else { cwd.clone() };
                    p.search_terms = vec!["kitty".into(), cwd.clone(), tab_title.clone()];
                    p.parent_id = tab_item.clone();
                    p.badges = vec!["Pane".into()];
                    p.active = active;
                    p.recency = if active { 3800.0 } else { 0.0 };
                    p.activation = activation.clone();
                    surfaces.push(json!({"provider": "kitty", "item_id": pane_item, "pid": pid, "title": title, "cwd": cwd, "address": host_address, "window_id": window_id, "activation": activation}));
                    result.items.push(p);
                }
            }
        }
    }
    result.metadata = json!({"surfaces": surfaces});
    Ok(result)
}

pub fn activate(activation: &Value, ctx: &mut Context) -> Res<()> {
    let socket = text(activation, "socket");
    let pid = num(activation, "pid");
    if process_birth(pid) != text(activation, "birth") {
        return Err("Kitty process was replaced".into());
    }
    let meta = std::fs::symlink_metadata(&socket).map_err(|_| "Kitty socket closed".to_string())?;
    if meta.uid() != current_uid() || !meta.file_type().is_socket() {
        return Err("Kitty socket is not a current user socket".into());
    }
    if activation.get("socket_dev").and_then(Value::as_u64) != Some(meta.dev()) || activation.get("socket_ino").and_then(Value::as_u64) != Some(meta.ino()) {
        return Err("Kitty socket was replaced".into());
    }
    let (command, identifier) = match text(activation, "target").as_str() {
        "tab" => ("focus-tab", num(activation, "tab_id")),
        "window" => ("focus-window", num(activation, "window_id")),
        _ => return Err("unknown Kitty activation target".into()),
    };
    if identifier <= 0 {
        return Err("invalid Kitty target id".into());
    }
    let (code, _, err) = run_cmd(&["kitten", "@", "--to", &format!("unix:{socket}"), command, "--match", &format!("id:{identifier}")], 1.2)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Kitty target is stale".into() } else { err.trim().into() });
    }
    let mut address = normalized_address(&text(activation, "address"));
    if address.is_empty() || ctx.client_by_address(&address).map(|c| num(c, "pid")) != Some(pid) {
        address = focused_address_for_pid(pid);
    }
    if address.is_empty() {
        return Err("Kitty focused its pane but its managed window could not be identified".into());
    }
    focus_address(&address)
}

fn focused_address_for_pid(pid: i64) -> String {
    for _ in 0..8 {
        if let Ok((0, out, _)) = run_cmd(&["hyprctl", "-j", "activewindow"], 0.5) {
            if let Ok(active) = serde_json::from_str::<Value>(&out) {
                if num(&active, "pid") == pid {
                    return normalized_address(&text(&active, "address"));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    String::new()
}
