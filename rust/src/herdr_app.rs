//! Herdr desktop app provider: spaces, tabs and agents from `herdr-app ls`.
//!
//! The app (herdr-app + herdr-core) has no per-session socket like the TUI, so
//! discovery shells out to `herdr-app ls` and `hc ls --json`; activation raises
//! the app window through Hyprland and runs `herdr-app do ...`.

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

pub fn scan(_ctx: &Context) -> Res<ProviderResult> {
    let (code, out, err) = run_cmd(&["herdr-app", "ls"], 1.5)?;
    if code != 0 {
        return Err(if err.trim().is_empty() { "Herdr app is not running".into() } else { err.trim().into() });
    }
    let listing: Value = serde_json::from_str(&out).map_err(|_| "herdr-app ls returned invalid JSON")?;
    // Terminal details (cwd) are optional; the listing alone is enough.
    let mut cwds: HashMap<String, String> = HashMap::new();
    if let Ok((0, hc_out, _)) = run_cmd(&["hc", "ls", "--json"], 1.5) {
        if let Ok(v) = serde_json::from_str::<Value>(&hc_out) {
            for t in arr(&v, "terminals") { cwds.insert(text(t, "address"), text(t, "cwd")); }
        }
    }
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
                let title = if !name.is_empty() { name } else if tab_name.is_empty() { pretty_agent(&app) } else { tab_name.clone() };
                let cwd = cwds.get(&address).cloned().unwrap_or_default();
                let mut a = Thing::new(stable_id("herdr-app", &["agent", &address]), "herdr-agent", "Herdr", &title);
                a.context = if cwd.is_empty() { space_name.clone() } else { format!("{space_name} · {cwd}") };
                a.search_terms = vec!["herdr".into(), "agent".into(), app.clone(), address.clone(), status.clone(), space_name.clone(), cwd];
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
    fn option_looking_ids_are_refused() {
        let mut ctx = Context::default();
        assert!(activate(&json!({"kind": "agent", "terminal": "--evil"}), &mut ctx).is_err());
        assert!(activate(&json!({"kind": "nope"}), &mut ctx).is_err());
    }
}
