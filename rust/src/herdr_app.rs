//! Herdr desktop app provider: spaces, tabs and agents from `herdr-app ls`.
//!
//! The app (herdr-app + herdr-core) has no per-session socket like the TUI, so
//! discovery shells out to `herdr-app ls` for the layout and to herdr-core for
//! each terminal's directory and pane title; activation raises the app window
//! through Hyprland and runs `herdr-app do ...`.

use crate::model::{stable_id, Thing};
use crate::providers::*;
use serde_json::{json, Value};
use std::collections::HashMap;

fn arr<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value.get(key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

fn pretty_agent(app: &str) -> String {
    match app {
        "claude-code" => "Claude Code".into(),
        "opencode" => "OpenCode".into(),
        other => other.split(['-', '_']).map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" "),
    }
}

fn status_label(status: &str) -> String {
    if status.is_empty() { "Unknown".into() } else { let mut c = status.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }
}

/// What herdr-core knows about one terminal: the directory its program runs in,
/// and the title that program paints. Agents put their own session title there,
/// so the pane, not the tab, is what names an agent.
#[derive(Default)]
struct TerminalFacts {
    cwd: String,
    title: String,
}

fn terminal_facts() -> HashMap<String, TerminalFacts> {
    let mut facts = HashMap::new();
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "terminal.list", "params": {}});
    if let Ok((0, out, _)) = run_cmd_input(&["hc", "connect"], &format!("{request}\n"), 1.5) {
        if let Ok(reply) = serde_json::from_str::<Value>(out.lines().next().unwrap_or_default()) {
            if let Some(result) = reply.get("result") {
                for t in arr(result, "terminals") {
                    facts.insert(text(t, "address"), TerminalFacts { cwd: text(t, "cwd"), title: text(t, "title") });
                }
            }
        }
    }
    if facts.is_empty() {
        // `hc ls` still has the directories when the protocol leaves them out.
        if let Ok((0, out, _)) = run_cmd(&["hc", "ls", "--json"], 1.5) {
            if let Ok(v) = serde_json::from_str::<Value>(&out) {
                for t in arr(&v, "terminals") { facts.insert(text(t, "address"), TerminalFacts { cwd: text(t, "cwd"), ..Default::default() }); }
            }
        }
    }
    facts
}

/// The program's own name for the pane, or empty when it says nothing a person
/// can use. OpenCode prefixes its title with "OC | "; a title that is only the
/// program's name, a path, a shell prompt with an account, a terminal address,
/// or a number (an agent that has not titled its session yet) is not a name.
fn pane_title(raw: &str, app: &str, address: &str) -> String {
    let trimmed = raw.trim();
    let title = trimmed.strip_prefix("OC | ").unwrap_or(trimmed).trim_start_matches(|c: char| !c.is_alphanumeric());
    let lower = title.to_lowercase();
    let pathish = matches!(title.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic());
    if title.is_empty() || pathish || title.contains('@') || title == address
        || title.chars().all(|c| c.is_ascii_digit())
        || lower == app.to_lowercase() || lower == pretty_agent(app).to_lowercase() {
        return String::new();
    }
    title.to_string()
}

pub fn scan(_ctx: &Context) -> Res<ProviderResult> {
    let (code, out, err) = run_cmd(&["herdr-app", "ls"], 1.5)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Herdr app is not running".into() } else { err.trim().into() });
    }
    let listing: Value = serde_json::from_str(&out).map_err(|_| "herdr-app ls returned invalid JSON")?;
    // Pane titles and directories are optional; the listing alone is enough.
    let facts = terminal_facts();
    let mut result = ProviderResult { provider: "herdr-app", ..Default::default() };
    let recent: Vec<String> = arr(&listing, "recent").iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect();
    for space in arr(&listing, "spaces") {
        let (space_id, space_name) = (text(space, "id"), text(space, "name"));
        if space_id.is_empty() { continue; }
        let space_active = space.get("active") == Some(&Value::Bool(true));
        let space_item = stable_id("herdr-app", &["space", &space_id]);
        let tabs = arr(space, "tabs");
        let mut s = Thing::new(space_item.clone(), "herdr-workspace", "Herdr", &space_name);
        s.context = format!("{} tabs", tabs.len());
        s.search_terms = vec!["herdr".into(), "space".into(), space_id.clone()];
        s.badges = vec!["Space".into()];
        s.active = space_active;
        s.recency = if space_active { 3400.0 } else { 0.0 };
        s.activation = json!({"kind": "space", "id": space_id});
        result.items.push(s);
        for tab in tabs {
            let (tab_id, tab_name) = (text(tab, "id"), text(tab, "name"));
            if tab_id.is_empty() { continue; }
            let tab_active = space_active && tab.get("active") == Some(&Value::Bool(true));
            let tab_item = stable_id("herdr-app", &["tab", &tab_id]);
            let terminals = arr(tab, "terminals");
            let mut t = Thing::new(tab_item.clone(), "herdr-tab", "Herdr", &tab_name);
            t.context = format!("{} · {} terminals", space_name, terminals.len());
            t.search_terms = vec!["herdr".into(), "tab".into(), tab_id.clone()];
            t.parent_id = space_item.clone();
            t.badges = vec!["Tab".into()];
            t.active = tab_active;
            t.recency = if tab_active { 3600.0 } else { 0.0 };
            t.activation = json!({"kind": "tab", "id": tab_id, "space": space_id});
            result.items.push(t);
            for term in terminals {
                let (address, app) = (text(term, "terminal"), text(term, "app"));
                if address.is_empty() || app.is_empty() { continue; }
                let status = text(term, "status");
                let name = text(term, "name");
                let (pane, cwd) = match facts.get(&address) {
                    Some(f) => (pane_title(&f.title, &app, &address), f.cwd.clone()),
                    None => (String::new(), String::new()),
                };
                // The agent's own session title, else its terminal name, else the tab.
                let title = if !pane.is_empty() { pane } else if !name.is_empty() { name } else if tab_name.is_empty() { pretty_agent(&app) } else { tab_name.clone() };
                let mut a = Thing::new(stable_id("herdr-app", &["agent", &address]), "herdr-agent", "Herdr", &title);
                a.context = if cwd.is_empty() { space_name.clone() } else { format!("{space_name} · {cwd}") };
                a.search_terms = vec!["herdr".into(), "agent".into(), app.clone(), address.clone(), status.clone(), title.clone(), space_name.clone(), cwd];
                a.parent_id = tab_item.clone();
                a.badges = vec![pretty_agent(&app), status_label(&status)];
                a.active = tab_active && recent.first().map(|r| *r == address).unwrap_or(false);
                a.recency = if tab_active { 3800.0 } else if status == "working" { 500.0 } else { 0.0 };
                a.activation = json!({"kind": "agent", "terminal": address});
                result.items.push(a);
            }
        }
    }
    Ok(result)
}

fn app_do(args: &[&str]) -> Res<()> {
    let mut argv = vec!["herdr-app", "do"];
    argv.extend_from_slice(args);
    let (code, _, err) = run_cmd(&argv, 3.0)?;
    if code != 0 {
        let msg = err.trim();
        return Err(if msg.is_empty() { "Herdr target closed".into() } else { msg.to_string() });
    }
    Ok(())
}

pub fn activate(activation: &Value, _ctx: &mut Context) -> Res<()> {
    let safe = |key: &str| -> Res<String> {
        let v = text(activation, key);
        if v.is_empty() || v.starts_with('-') || v.contains(['\0', '\n', '\r']) { Err(format!("Herdr target has no exact {key}")) } else { Ok(v) }
    };
    // Raise the window first (Hyprland switches workspace); start the app if it is gone.
    if run_cmd(&["herdr-app-active", "--focus"], 2.0).map(|r| r.0 != 0).unwrap_or(true) {
        let _ = run_cmd(&["herdr-app-open"], 2.0);
    }
    match text(activation, "kind").as_str() {
        "agent" => app_do(&["agent.focus", "--terminal", &safe("terminal")?]),
        "space" => app_do(&["space.select", "--id", &safe("id")?]),
        "tab" => {
            if let Ok(space) = safe("space") { app_do(&["space.select", "--id", &space])?; }
            app_do(&["tab.select", "--id", &safe("id")?])
        }
        _ => Err("Unknown Herdr activation target".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_names_are_readable() {
        assert_eq!(pretty_agent("claude-code"), "Claude Code");
        assert_eq!(pretty_agent("opencode"), "OpenCode");
        assert_eq!(pretty_agent("some-new_agent"), "Some New Agent");
        assert_eq!(status_label("working"), "Working");
        assert_eq!(status_label(""), "Unknown");
    }

    #[test]
    fn pane_titles_name_the_agent() {
        // The program's own session title wins over the tab name.
        assert_eq!(pane_title("OC | Everything plugin pane titles for agents", "opencode", "t31"), "Everything plugin pane titles for agents");
        assert_eq!(pane_title("Omarchy and herdr theme alignment", "claude-code", "t3r"), "Omarchy and herdr theme alignment");
        // Unset: the title is the program's own name, a path, a shell prompt with
        // an account, the address, or a number.
        assert_eq!(pane_title("opencode", "opencode", "t31"), "");
        assert_eq!(pane_title("OpenCode", "opencode", "t31"), "");
        assert_eq!(pane_title("adhikasp@semar:~/pi-life", "bash", "t34"), "");
        assert_eq!(pane_title("C:\\Workspace\\app", "bash", "t34"), "");
        assert_eq!(pane_title("t34", "bash", "t34"), "");
        assert_eq!(pane_title("", "opencode", "t31"), "");
        assert_eq!(pane_title("  #  ", "opencode", "t31"), "");
    }

    #[test]
    fn option_looking_ids_are_refused() {
        let mut ctx = Context::default();
        assert!(activate(&json!({"kind": "agent", "terminal": "--evil"}), &mut ctx).is_err());
        assert!(activate(&json!({"kind": "nope"}), &mut ctx).is_err());
    }
}
