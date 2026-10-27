//! tmux sessions, windows and panes through the CLI on discovered sockets.

use crate::model::{process_birth, stable_id, Thing};
use crate::procs::{basename, canonical_path, current_uid, proc_cwd};
use crate::providers::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::os::unix::fs::{FileTypeExt, MetadataExt};

const SEP: char = '\x1f';

fn native_id(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('$' | '@' | '%')) && value.len() > 1 && chars.all(|c| c.is_ascii_digit())
}

fn is_socket(path: &str) -> bool {
    std::fs::symlink_metadata(path).map(|m| m.uid() == current_uid() && m.file_type().is_socket()).unwrap_or(false)
}

fn discover_sockets(ctx: &Context) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(format!("/tmp/tmux-{}", current_uid())) {
        candidates.extend(entries.flatten().map(|e| e.path().to_string_lossy().into_owned()));
    }
    if let Some(runtime) = std::env::var("XDG_RUNTIME_DIR").ok().filter(|r| !r.is_empty()) {
        if let Ok(entries) = std::fs::read_dir(runtime) {
            candidates.extend(entries.flatten().filter(|e| e.file_name().to_string_lossy().to_lowercase().contains("tmux")).map(|e| e.path().to_string_lossy().into_owned()));
        }
    }
    for process in ctx.procs.named("tmux") {
        for (i, arg) in process.argv.iter().enumerate().take(process.argv.len().saturating_sub(1)) {
            if arg == "-S" {
                candidates.insert(process.argv[i + 1].clone());
            }
        }
    }
    candidates.into_iter().filter(|c| is_socket(c)).filter_map(|c| std::fs::canonicalize(&c).ok()).map(|p| p.to_string_lossy().into_owned()).collect::<BTreeSet<_>>().into_iter().collect()
}

fn records(socket: &str, command: &[&str], fields: &[&str]) -> Res<Vec<Vec<String>>> {
    let format = fields.iter().map(|f| format!("#{{{f}}}")).collect::<Vec<_>>().join(&SEP.to_string());
    let mut argv = vec!["tmux", "-S", socket];
    argv.extend_from_slice(command);
    argv.extend_from_slice(&["-F", &format]);
    let (code, out, err) = run_cmd(&argv, 1.0)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "tmux query failed".into() } else { err.trim().into() });
    }
    Ok(out.lines().map(|l| l.split(SEP).map(String::from).collect::<Vec<_>>()).filter(|p| p.len() == fields.len()).collect())
}

const CLIENT_FIELDS: [&str; 4] = ["client_name", "client_pid", "client_session", "client_tty"];

fn activation(socket: &str, server_pid: i64, birth: &str, target: &str, kind: &str, clients: &[Vec<String>], session: &str) -> Value {
    json!({"socket": socket, "server_pid": server_pid, "birth": birth, "target_id": target, "target_kind": kind, "session_id": session, "clients": clients})
}

pub fn scan(ctx: &Context) -> Res<ProviderResult> {
    let mut result = ProviderResult { provider: "tmux", ..Default::default() };
    let (mut panes_meta, mut sockets_meta) = (Vec::new(), Vec::new());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let recent = |value: &str, bonus: f64| {
        let age = value.parse::<f64>().map(|v| (now - v).max(0.0)).unwrap_or(1000.0);
        bonus + (1000.0 - age.min(1000.0)).max(0.0)
    };
    for socket in discover_sockets(ctx) {
        let queried = (|| -> Res<_> {
            Ok((
                records(&socket, &["list-sessions"], &["pid", "session_id", "session_name", "session_attached", "session_activity"])?,
                records(&socket, &["list-windows", "-a"], &["session_id", "window_id", "window_name", "window_active", "window_activity"])?,
                records(&socket, &["list-panes", "-a"], &["session_id", "window_id", "pane_id", "pane_title", "pane_current_path", "pane_active", "pane_pid"])?,
                records(&socket, &["list-clients"], &CLIENT_FIELDS)?,
            ))
        })();
        let (sessions, windows, panes, clients) = match queried {
            Ok(rows) => rows,
            Err(error) => {
                result.warnings.push(format!("tmux socket {}: {error}", basename(&socket)));
                continue;
            }
        };
        let server_pid = sessions.first().and_then(|s| s[0].parse::<i64>().ok()).unwrap_or(0);
        let birth = process_birth(server_pid);
        let id_for = |native: &str| stable_id("tmux", &[&socket, &server_pid.to_string(), &birth, native]);
        let (mut session_ids, mut window_ids) = (std::collections::HashMap::new(), std::collections::HashMap::new());
        for row in &sessions {
            let (native, name, attached, activity) = (&row[1], &row[2], &row[3], &row[4]);
            if !native_id(native) { continue; }
            session_ids.insert(native.clone(), id_for(native));
            let mut t = Thing::new(id_for(native), "tmux-session", "tmux", if name.is_empty() { native } else { name });
            t.context = format!("{attached} attached client{}", if attached != "1" { "s" } else { "" });
            t.search_terms = vec!["tmux".into(), socket.clone(), native.clone()];
            t.badges = vec!["Session".into(), if attached != "0" { "Attached".into() } else { "Detached".into() }];
            t.recency = recent(activity, 0.0);
            t.activation = activation(&socket, server_pid, &birth, native, "session", &clients, native);
            result.items.push(t);
        }
        for row in &windows {
            let (session_id, native, name, active, activity) = (&row[0], &row[1], &row[2], &row[3], &row[4]);
            if !native_id(native) { continue; }
            window_ids.insert(native.clone(), id_for(native));
            let mut t = Thing::new(id_for(native), "tmux-window", "tmux", if name.is_empty() { native } else { name });
            t.context = format!("tmux window {native}");
            t.search_terms = vec!["tmux".into(), session_id.clone(), native.clone()];
            t.parent_id = session_ids.get(session_id).cloned().unwrap_or_default();
            t.badges = vec!["Window".into()];
            t.active = active == "1";
            t.recency = recent(activity, if t.active { 3000.0 } else { 0.0 });
            t.activation = activation(&socket, server_pid, &birth, native, "window", &clients, session_id);
            result.items.push(t);
        }
        for row in &panes {
            let (session_id, window_id, native, title, cwd, active, pane_pid) = (&row[0], &row[1], &row[2], &row[3], &row[4], &row[5], &row[6]);
            if !native_id(native) { continue; }
            let path = canonical_path(cwd);
            let mut act = activation(&socket, server_pid, &birth, native, "pane", &clients, session_id);
            act["pane_pid"] = json!(pane_pid.parse::<i64>().unwrap_or(0));
            let shown = [title.clone(), basename(&path).to_string(), native.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            let mut t = Thing::new(id_for(native), "tmux-pane", "tmux", &shown);
            t.context = if path.is_empty() { format!("tmux pane {native}") } else { path.clone() };
            t.search_terms = vec!["tmux".into(), session_id.clone(), window_id.clone(), native.clone(), path.clone()];
            t.parent_id = window_ids.get(window_id).cloned().unwrap_or_default();
            t.badges = vec!["Pane".into()];
            t.active = active == "1";
            t.recency = if t.active { 3200.0 } else { 0.0 };
            t.activation = act.clone();
            panes_meta.push(json!({"item_id": t.id, "socket": socket, "pane_id": native, "pane_pid": pane_pid.parse::<i64>().unwrap_or(0), "cwd": path, "title": title, "activation": act}));
            result.items.push(t);
        }
        sockets_meta.push(json!({"socket": socket, "server_pid": server_pid, "birth": birth, "clients": clients}));
    }
    result.metadata = json!({"panes": panes_meta, "sockets": sockets_meta});
    Ok(result)
}

/// tmux client rows whose process is a local tmux, with their window route.
fn local_clients(ctx: &Context, clients: &[Vec<String>]) -> Vec<(String, i64)> {
    clients
        .iter()
        .filter(|row| row.len() >= 2)
        .filter_map(|row| {
            let pid = row[1].parse::<i64>().ok()?;
            let process = ctx.procs.get(pid)?;
            let is_tmux = process.comm.eq_ignore_ascii_case("tmux") || process.argv.first().map(|a| basename(a).eq_ignore_ascii_case("tmux")).unwrap_or(false);
            is_tmux.then(|| (row[0].clone(), pid))
        })
        .collect()
}

pub fn activate(activation: &Value, ctx: &mut Context) -> Res<()> {
    let socket = text(activation, "socket");
    if process_birth(num(activation, "server_pid")) != text(activation, "birth") {
        return Err("tmux server was replaced".into());
    }
    if std::fs::symlink_metadata(&socket).is_err() {
        return Err("tmux socket closed".into());
    }
    if !is_socket(&socket) {
        return Err("tmux socket is not owned by the current user".into());
    }
    let target = text(activation, "target_id");
    if !native_id(&target) {
        return Err("invalid tmux native target".into());
    }
    let live = records(&socket, &["list-clients"], &CLIENT_FIELDS)?;
    let routes: Vec<(String, Value)> = local_clients(ctx, &live)
        .into_iter()
        .filter_map(|(name, pid)| route_for_process(ctx, pid, "", &proc_cwd(pid)).map(|r| (name, r)))
        .collect();
    let switch = |client: &str| -> Res<()> {
        let (code, _, err) = run_cmd(&["tmux", "-S", &socket, "switch-client", "-c", client, "-t", &target], 1.0)?;
        if code != 0 {
            return Err(if err.trim().is_empty() { "tmux target closed".into() } else { err.trim().into() });
        }
        Ok(())
    };
    if routes.len() == 1 {
        let (client, route) = &routes[0];
        switch(client)?;
        let provider = text(route, "provider");
        return activate_provider(&provider, &route.get("activation").cloned().unwrap_or(json!({})), ctx);
    }
    if activation.get("allow_new_client") == Some(&Value::Bool(false)) {
        if let Some((client, _)) = local_clients(ctx, &live).first() {
            switch(client)?;
        }
        return Ok(());
    }
    let session = text(activation, "session_id");
    if !native_id(&session) || !session.starts_with('$') {
        return Err("tmux target has no live session".into());
    }
    let mut argv: Vec<String> = ["omarchy-launch-terminal", "tmux", "-S", &socket, "attach-session", "-t", &session].iter().map(|s| s.to_string()).collect();
    if target.starts_with('@') {
        argv.extend([";".into(), "select-window".into(), "-t".into(), target.clone()]);
    } else if target.starts_with('%') {
        argv.extend([";".into(), "select-pane".into(), "-t".into(), target.clone()]);
    }
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    launch_terminal_and_focus(&refs)
}
