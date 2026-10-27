//! Neovim buffers through `nvim --server <socket> --remote-expr`.

use crate::model::{process_birth, stable_id, Thing};
use crate::procs::{basename, canonical_path, current_uid, username};
use crate::providers::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::os::unix::fs::{FileTypeExt, MetadataExt};

const QUERY_EXPR: &str = "luaeval('vim.json.encode({pid=vim.fn.getpid(),current=vim.api.nvim_get_current_buf(),cwd=vim.fn.getcwd(),terminal_title=(vim.o.title and vim.o.titlestring or \"\"),buffers=vim.tbl_map(function(b) return {bufnr=b.bufnr,name=b.name,changed=b.changed,loaded=b.loaded,listed=b.listed,windows=vim.fn.win_findbuf(b.bufnr),lastused=b.lastused,changedtick=vim.api.nvim_buf_get_changedtick(b.bufnr)} end,vim.fn.getbufinfo({buflisted=1}))})')";

/// Sockets directly below a run dir or one private tempdir; same-user, bounded.
fn runtime_sockets(root: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let Ok(meta) = std::fs::symlink_metadata(root) else { return found };
    if meta.uid() != current_uid() || !meta.is_dir() {
        return found;
    }
    let (mut pending, mut inspected) = (vec![(root.to_string(), 0)], 0);
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            inspected += 1;
            if inspected > 4096 { return found; }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
            if meta.uid() != current_uid() { continue; }
            let path = entry.path().to_string_lossy().into_owned();
            if meta.file_type().is_socket() && entry.file_name().to_string_lossy().starts_with("nvim") {
                found.insert(path);
            } else if depth == 0 && meta.is_dir() {
                pending.push((path, 1));
            }
        }
    }
    found
}

fn discover_sockets(ctx: &Context) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", current_uid()));
    let temp = std::env::var("TMPDIR").unwrap_or("/tmp".into());
    for root in [runtime, format!("{temp}/nvim.{}", username())] {
        candidates.extend(runtime_sockets(&root));
    }
    for name in ["nvim", "neovim"] {
        for process in ctx.procs.named(name) {
            for (i, arg) in process.argv.iter().enumerate().take(process.argv.len().saturating_sub(1)) {
                if arg == "--listen" {
                    candidates.insert(process.argv[i + 1].clone());
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter(|c| std::fs::symlink_metadata(c).map(|m| m.uid() == current_uid() && m.file_type().is_socket()).unwrap_or(false))
        .filter_map(|c| std::fs::canonicalize(c).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn query(socket: &str) -> Res<Value> {
    let (code, out, err) = run_cmd(&["nvim", "--server", socket, "--remote-expr", QUERY_EXPR], 1.2)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Neovim socket did not answer".into() } else { err.trim().into() });
    }
    let mut value: Value = serde_json::from_str(out.trim()).map_err(|_| "Neovim returned invalid buffer JSON")?;
    if let Value::String(inner) = &value {
        value = serde_json::from_str(inner).map_err(|_| "Neovim returned invalid buffer JSON")?;
    }
    if value.is_object() && value.get("buffers").map(Value::is_array).unwrap_or(false) {
        Ok(value)
    } else {
        Err("Neovim buffer response has the wrong shape".into())
    }
}

fn f64_of(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// (route, parent item id, ambiguous ghostty)
fn route(ctx: &Context, pid: i64, process_cwd: &str, terminal_title: &str) -> (Value, String, bool) {
    let remote = || json!({"provider": "remote-ui"});
    let tmux: Vec<Value> = ctx.meta_list("tmux", "panes").into_iter().filter(|p| num(p, "pane_pid") != 0 && ctx.procs.is_descendant(pid, num(p, "pane_pid"))).collect();
    if tmux.len() == 1 {
        return (json!({"provider": "tmux", "activation": tmux[0].get("activation").cloned().unwrap_or(json!({}))}), text(&tmux[0], "item_id"), false);
    }
    if tmux.len() > 1 {
        return (remote(), String::new(), false);
    }

    let cwd = canonical_path(process_cwd);
    let herdr_panes = ctx.meta_list("herdr", "panes");
    let sessions: Vec<Value> = ctx.meta_list("herdr", "sessions").into_iter().filter(|s| num(s, "server_pid") != 0 && ctx.procs.is_descendant(pid, num(s, "server_pid"))).collect();
    let server_pids: Vec<i64> = sessions.iter().map(|s| num(s, "server_pid")).collect();
    let env = ctx.procs.environment(pid, &["HERDR_SOCKET_PATH", "HERDR_PANE_ID"]);
    let (env_socket, env_pane) = (env.get("HERDR_SOCKET_PATH").cloned().unwrap_or_default(), env.get("HERDR_PANE_ID").cloned().unwrap_or_default());
    let by_env: Vec<&Value> = herdr_panes.iter().filter(|p| text(p, "socket") == env_socket && text(p, "pane_id") == env_pane && server_pids.contains(&num(p, "server_pid"))).collect();
    let herdr_route = |pane: &Value| (json!({"provider": "herdr", "activation": pane.get("activation").cloned().unwrap_or(json!({}))}), text(pane, "item_id"), false);
    if by_env.len() == 1 {
        return herdr_route(by_env[0]);
    }
    let by_cwd: Vec<&Value> = herdr_panes.iter().filter(|p| server_pids.contains(&num(p, "server_pid")) && !cwd.is_empty() && canonical_path(&text(p, "cwd")) == cwd).collect();
    if by_cwd.len() == 1 {
        return herdr_route(by_cwd[0]);
    }
    if !sessions.is_empty() {
        return (remote(), String::new(), false);
    }

    for provider in ["kitty"] {
        let surfaces: Vec<Value> = ctx.meta_list(provider, "surfaces").into_iter().filter(|s| num(s, "pid") != 0 && ctx.procs.is_descendant(pid, num(s, "pid"))).collect();
        if surfaces.is_empty() {
            continue;
        }
        let matches: Vec<&Value> = if surfaces.len() == 1 {
            surfaces.iter().collect()
        } else {
            surfaces.iter().filter(|s| !cwd.is_empty() && canonical_path(&text(s, "cwd")) == cwd && !terminal_title.is_empty() && text(s, "title") == terminal_title).collect()
        };
        if matches.len() == 1 {
            return (json!({"provider": provider, "activation": matches[0].get("activation").cloned().unwrap_or(json!({}))}), text(matches[0], "item_id"), false);
        }
        return (remote(), String::new(), false);
    }

    if let Some(ancestor) = ctx.procs.ancestor_pid_in(pid, &ctx.managed_pids()) {
        let clients = ctx.clients_for_pid(ancestor);
        if clients.len() == 1 {
            let address = normalized_address(&text(clients[0], "address"));
            let client_pid = num(clients[0], "pid");
            return (
                json!({"provider": "hyprland", "activation": {"address": address, "pid": client_pid, "birth": process_birth(client_pid)}}),
                ctx.window_item_id(&address),
                false,
            );
        }
    }
    (remote(), String::new(), false)
}

pub fn scan(ctx: &Context) -> Res<ProviderResult> {
    let mut result = ProviderResult { provider: "neovim", ..Default::default() };
    let mut servers = Vec::new();
    for socket in discover_sockets(ctx) {
        let short = basename(&socket).to_string();
        let response = match query(&socket) {
            Ok(r) => r,
            Err(error) => {
                result.warnings.push(format!("Neovim socket {short}: {error}"));
                continue;
            }
        };
        let pid = num(&response, "pid");
        let process = ctx.procs.get(pid);
        let executable = process.and_then(|p| p.argv.first()).map(|a| basename(a).to_lowercase()).unwrap_or_default();
        let ok = process.map(|p| ["nvim", "neovim"].contains(&p.comm.to_lowercase().as_str()) || ["nvim", "neovim"].contains(&executable.as_str())).unwrap_or(false);
        if !ok {
            result.warnings.push(format!("Neovim socket {short} did not identify a current-user Nvim process"));
            continue;
        }
        let birth = process.map(|p| p.start_time.clone()).unwrap_or_default();
        let current = num(&response, "current");
        let editor_cwd = canonical_path(&Some(text(&response, "cwd")).filter(|c| !c.is_empty()).unwrap_or_else(|| crate::procs::proc_cwd(pid)));
        let terminal_title = text(&response, "terminal_title");
        let (route, parent_id, ambiguous) = route(ctx, pid, &editor_cwd, &terminal_title);
        let buffers: Vec<&Value> = response["buffers"].as_array().into_iter().flatten().filter(|b| b.is_object()).collect();
        let newest = buffers.iter().map(|b| f64_of(b, "lastused")).fold(0.0, f64::max);
        for buffer in buffers {
            if buffer.get("listed") == Some(&Value::Bool(false)) { continue; }
            let bufnr = num(buffer, "bufnr");
            if bufnr <= 0 { continue; }
            let name = canonical_path(&text(buffer, "name"));
            let title = if name.is_empty() { format!("[No Name {bufnr}]") } else { basename(&name).to_string() };
            let directory = if name.is_empty() { editor_cwd.clone() } else { std::path::Path::new(&name).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() };
            let changed = buffer.get("changed").map(|v| v == &Value::Bool(true) || v == &json!(1)).unwrap_or(false);
            let loaded = buffer.get("loaded").map(|v| v == &Value::Bool(true) || v == &json!(1)).unwrap_or(false);
            let visible = buffer.get("windows").and_then(Value::as_array).map(|w| w.iter().any(|v| v.as_u64().is_some())).unwrap_or(false);
            let mut badges = vec!["Buffer".to_string()];
            if changed { badges.push("Modified".into()); }
            if !loaded { badges.push("Unloaded".into()); }
            if visible { badges.push("Visible".into()); }
            let recent = (1000.0 - (newest - f64_of(buffer, "lastused")).min(1000.0)).max(0.0);
            let mut t = Thing::new(
                stable_id("neovim", &[&socket, &pid.to_string(), &birth, &bufnr.to_string(), if name.is_empty() { "unnamed" } else { &name }]),
                "neovim-buffer",
                "Neovim",
                &format!("{title}{}", if changed { " [+]" } else { "" }),
            );
            t.context = if directory.is_empty() { format!("Neovim server {short}") } else { directory };
            t.search_terms = vec!["neovim".into(), "nvim".into(), name.clone(), bufnr.to_string()];
            t.parent_id = parent_id.clone();
            t.badges = badges;
            t.active = bufnr == current;
            t.recency = (if bufnr == current { 3900.0 } else { 0.0 }) + recent;
            t.activation = json!({"socket": socket, "pid": pid, "birth": birth, "bufnr": bufnr, "name": name, "route": route, "ambiguous_ghostty": ambiguous});
            result.items.push(t);
        }
        servers.push(json!({"socket": socket, "pid": pid, "birth": birth, "route": route}));
    }
    result.metadata = json!({"servers": servers});
    Ok(result)
}

pub fn activate(activation: &Value, ctx: &mut Context) -> Res<()> {
    let socket = text(activation, "socket");
    let pid = num(activation, "pid");
    if process_birth(pid) != text(activation, "birth") {
        return Err("Neovim server was replaced".into());
    }
    let meta = std::fs::symlink_metadata(&socket).map_err(|_| "Neovim socket closed".to_string())?;
    if meta.uid() != current_uid() || !meta.file_type().is_socket() {
        return Err("Neovim socket is not a current-user socket".into());
    }
    let bufnr = num(activation, "bufnr");
    let current = query(&socket)?;
    if num(&current, "pid") != pid {
        return Err("Neovim socket was replaced".into());
    }
    let buffer = current["buffers"].as_array().into_iter().flatten().find(|b| b.is_object() && num(b, "bufnr") == bufnr);
    if buffer.map(|b| canonical_path(&text(b, "name"))) != Some(text(activation, "name")) {
        return Err("Neovim buffer closed or its number was reused".into());
    }
    // Reach an already displayed buffer (even on another tab page); otherwise
    // :hide lets us switch without discarding a modified buffer.
    let expression = format!(
        "luaeval('(function(b) local w=vim.fn.win_findbuf(b); if #w>0 then vim.api.nvim_set_current_win(w[1]); return 1 else vim.cmd(\"hide buffer \"..b); return 0 end end)(_A)',{bufnr})"
    );
    let (code, out, err) = run_cmd(&["nvim", "--server", &socket, "--remote-expr", &expression], 1.2)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Neovim refused the buffer switch".into() } else { err.trim().into() });
    }
    let used_existing_window = match out.trim() {
        "1" => true,
        "0" => false,
        _ => return Err("Neovim returned an invalid buffer switch result".into()),
    };
    let route = activation.get("route").filter(|r| r.is_object()).cloned().unwrap_or(json!({}));
    let provider = Some(text(&route, "provider")).filter(|p| !p.is_empty()).unwrap_or("remote-ui".into());
    if provider != "remote-ui" {
        let mut route_activation = route.get("activation").cloned().unwrap_or(json!({}));
        if used_existing_window {
            route_activation["allow_new_client"] = json!(false);
        }
        activate_provider(&provider, &route_activation, ctx)?;
    } else if !used_existing_window {
        launch_terminal_and_focus(&["omarchy-launch-terminal", "nvim", "--server", &socket, "--remote-ui"])?;
    }
    Ok(())
}
