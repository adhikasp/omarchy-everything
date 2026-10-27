//! AT-SPI and Ghostty run in the Python helper (libatspi has no Rust
//! equivalent worth porting blind). It is started lazily, only when a window
//! that needs it is open, so the common path never pays Python's startup.

use crate::providers::{text, Context, Res};
use crate::{emit, Shared};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PROVIDERS: [&str; 2] = ["atspi", "ghostty"];

pub struct Sidecar {
    stdin: ChildStdin,
    child: Child,
    ready: Receiver<()>,
    done: Receiver<String>,
    /// Request id the reader may relay partials for.
    current: Arc<Mutex<String>>,
}

static SIDECAR: Mutex<Option<Sidecar>> = Mutex::new(None);

fn python_helper() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    // <plugin>/rust/target/release/everything-helper
    let root = exe.ancestors().nth(4)?;
    let path = root.join("helper/everything_helper.py");
    path.exists().then(|| path.to_string_lossy().into_owned())
}

const BROWSERS: [&str; 10] = ["chromium", "google-chrome", "chrome", "brave-browser", "brave", "microsoft-edge", "firefox", "zen", "vivaldi", "librewolf"];

fn is_browser_class(class: &str) -> bool {
    let class = class.to_lowercase();
    if class.ends_with("-default") && class.contains('-') && BROWSERS.iter().any(|b| class.starts_with(&format!("{b}-"))) {
        return false; // app-mode window
    }
    let is_sep = |c: char| matches!(c, '.' | '_' | '-');
    BROWSERS.iter().chain(["helium"].iter()).any(|name| {
        class.match_indices(name).any(|(at, _)| {
            let before = class[..at].chars().next_back();
            let after = class[at + name.len()..].chars().next();
            before.map_or(true, is_sep) && after.map_or(true, is_sep)
        })
    })
}

/// Whether any open window needs AT-SPI (or the user asked for Ghostty panes).
pub fn needed(ctx: &Context, ghostty_requested: bool) -> bool {
    if ghostty_requested {
        return true;
    }
    let extension = crate::providers::chrome_extension_available();
    ctx.clients.iter().any(|client| {
        let class = [text(client, "class"), text(client, "initialClass")].into_iter().find(|s| !s.is_empty()).unwrap_or_default().to_lowercase();
        if extension && (class.starts_with("google-chrome") || class.starts_with("chromium")) {
            return false;
        }
        is_browser_class(&class) || class == "com.github.pintaproject.pinta" || class == "org.gnome.nautilus"
    })
}

fn start(state: &Shared) -> Res<Sidecar> {
    let helper = python_helper().ok_or("AT-SPI sidecar is unavailable: Python helper not found")?;
    let mut child = Command::new("python3")
        .args([&helper, "--json-lines"])
        .env("EVERYTHING_ONLY_PROVIDERS", PROVIDERS.join(","))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("AT-SPI sidecar failed to start: {e}"))?;
    let stdin = child.stdin.take().ok_or("sidecar stdin missing")?;
    let stdout = child.stdout.take().ok_or("sidecar stdout missing")?;
    let (ready_tx, ready) = channel();
    let (done_tx, done) = channel();
    let current = Arc::new(Mutex::new(String::new()));
    let (state, relay_current) = (state.clone(), current.clone());
    std::thread::spawn(move || relay(BufReader::new(stdout), state, relay_current, ready_tx, done_tx));
    Ok(Sidecar { stdin, child, ready, done, current })
}

fn relay(reader: impl BufRead, state: Shared, current: Arc<Mutex<String>>, ready: Sender<()>, done: Sender<String>) {
    for line in reader.lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let request_id = text(&message, "requestId");
        match text(&message, "type").as_str() {
            "ready" => { let _ = ready.send(()); }
            "activation" => emit(message),
            "snapshot" => {
                if message.get("full") == Some(&Value::Bool(true)) {
                    let _ = done.send(request_id);
                    continue;
                }
                let provider = text(&message, "provider");
                if !PROVIDERS.contains(&provider.as_str()) || *current.lock().unwrap() != request_id {
                    continue;
                }
                let items: Vec<Value> = message.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
                let warnings: Vec<String> = message.get("warnings").and_then(Value::as_array).map(|w| w.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                {
                    let mut guard = state.lock().unwrap();
                    guard.external.insert(provider.clone(), items.clone());
                    guard.warnings.insert(provider.clone(), warnings.clone());
                }
                emit(json!({"version": 1, "type": "snapshot", "requestId": request_id, "provider": provider, "full": false, "items": items, "warnings": warnings}));
            }
            "error" if PROVIDERS.iter().any(|p| text(&message, "provider") == *p) => emit(message),
            _ => {}
        }
    }
}

/// Forward a scan and wait (bounded) for the sidecar's full snapshot.
pub fn scan(state: &Shared, request_id: &str, options: &Value) {
    let mut slot = SIDECAR.lock().unwrap();
    if slot.as_mut().map(|s| s.child.try_wait().ok().flatten().is_some()).unwrap_or(false) {
        *slot = None;
    }
    if slot.is_none() {
        match start(state) {
            Ok(sidecar) => {
                if sidecar.ready.recv_timeout(Duration::from_secs(5)).is_err() {
                    state.lock().unwrap().warnings.insert("atspi".into(), vec!["AT-SPI: sidecar did not start".into()]);
                    return;
                }
                *slot = Some(sidecar);
            }
            Err(error) => {
                state.lock().unwrap().warnings.insert("atspi".into(), vec![format!("AT-SPI: {error}")]);
                return;
            }
        }
    }
    let sidecar = slot.as_mut().unwrap();
    *sidecar.current.lock().unwrap() = request_id.to_string();
    let request = json!({"version": 1, "id": request_id, "type": "scan", "options": options});
    if writeln!(sidecar.stdin, "{request}").and_then(|_| sidecar.stdin.flush()).is_err() {
        *slot = None;
        return;
    }
    let done = &sidecar.done;
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match done.recv_timeout(left) {
            Ok(id) if id == request_id => return,
            Ok(_) => continue,
            Err(_) => return,
        }
    }
}

/// Forward an activation for a sidecar-owned item. False if the sidecar is gone.
pub fn forward(line: &Value) -> bool {
    let mut slot = SIDECAR.lock().unwrap();
    match slot.as_mut() {
        Some(sidecar) => writeln!(sidecar.stdin, "{line}").and_then(|_| sidecar.stdin.flush()).is_ok(),
        None => false,
    }
}

pub fn shutdown() {
    if let Some(mut sidecar) = SIDECAR.lock().unwrap().take() {
        let _ = writeln!(sidecar.stdin, "{}", json!({"version": 1, "id": "shutdown", "type": "shutdown"}));
        drop(sidecar.stdin);
        let _ = sidecar.child.wait();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_classes_need_word_boundaries_and_skip_app_mode() {
        assert!(is_browser_class("firefox"));
        assert!(is_browser_class("org.mozilla.firefox"));
        assert!(is_browser_class("Brave-browser"));
        assert!(!is_browser_class("chrome-youtube.com__-Default"));
        assert!(!is_browser_class("com.mitchellh.ghostty"));
        assert!(!is_browser_class("zenity"));
    }
}
