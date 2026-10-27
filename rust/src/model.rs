//! Things, opaque activation tokens and the per-provider snapshot registry.
//! Mirrors `everything/model.py`; the wire format is identical.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::io::Read;
use uuid::Uuid;

const UI_NAMESPACE: Uuid = Uuid::from_bytes([
    0x40, 0xad, 0x37, 0xf5, 0xb4, 0x88, 0x50, 0xbc, 0xb3, 0xad, 0xd3, 0x10, 0x96, 0x7b, 0x3f, 0xd2,
]);

/// Percent-encode like Python's `quote(value, safe="-._~")`.
pub fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn stable_id(provider: &str, parts: &[&str]) -> String {
    let joined: Vec<String> = parts.iter().map(|part| quote(part)).collect();
    format!("{provider}:{}", joined.join("/"))
}

fn ui_uuid(provider: &str, kind: &str, id: &str) -> String {
    let traits = serde_json::to_string(&json!([provider, kind, id])).unwrap_or_default();
    Uuid::new_v5(&UI_NAMESPACE, traits.as_bytes()).to_string()
}

/// Linux start tick: immutable per process, makes ids reuse-safe.
pub fn process_birth(pid: i64) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|data| {
            let rest = &data[data.rfind(')')? + 2..];
            rest.split_whitespace().nth(19).map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn unique(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let text = value.trim().to_string();
        if !text.is_empty() && !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

#[derive(Clone)]
pub struct Thing {
    pub id: String,
    pub kind: &'static str,
    pub provider: &'static str,
    pub title: String,
    pub icon_hints: Vec<String>,
    pub context: String,
    pub search_terms: Vec<String>,
    pub parent_id: String,
    pub badges: Vec<String>,
    pub active: bool,
    pub recency: f64,
    pub activation: Value,
}

impl Thing {
    pub fn new(id: String, kind: &'static str, provider: &'static str, title: &str) -> Self {
        let title = title.trim();
        Thing {
            id,
            kind,
            provider,
            title: if title.is_empty() { "Untitled".into() } else { title.into() },
            icon_hints: vec![],
            context: String::new(),
            search_terms: vec![],
            parent_id: String::new(),
            badges: vec![],
            active: false,
            recency: 0.0,
            activation: json!({}),
        }
    }

    fn normalized(mut self) -> Self {
        self.icon_hints = unique(self.icon_hints);
        self.search_terms = unique(self.search_terms);
        self.badges = unique(self.badges);
        self.context = self.context.trim().to_string();
        self
    }

    fn public(&self, token: &str) -> Value {
        json!({
            "id": self.id,
            "uiUuid": ui_uuid(self.provider, self.kind, &self.id),
            "kind": self.kind,
            "provider": self.provider,
            "title": self.title,
            "iconHints": self.icon_hints,
            "context": self.context,
            "searchTerms": self.search_terms,
            "parentId": self.parent_id,
            "badges": self.badges,
            "active": self.active,
            "recency": self.recency,
            "activationToken": token,
        })
    }
}

/// Merge equal rows; give distinct targets with a colliding id a stable suffix.
fn dedupe(items: Vec<Thing>) -> Vec<Thing> {
    let mut out: Vec<Thing> = Vec::new();
    for item in items.into_iter().map(Thing::normalized) {
        match out.iter().position(|existing| existing.id == item.id) {
            None => out.push(item),
            Some(position) => {
                let existing = &mut out[position];
                if existing.activation == item.activation && existing.kind == item.kind {
                    existing.icon_hints = unique([existing.icon_hints.clone(), item.icon_hints].concat());
                    existing.search_terms = unique([existing.search_terms.clone(), item.search_terms].concat());
                    existing.badges = unique([existing.badges.clone(), item.badges].concat());
                    existing.active |= item.active;
                    existing.recency = existing.recency.max(item.recency);
                } else {
                    let mut count = 2;
                    let base = item.id.clone();
                    let mut id = stable_id(item.provider, &["collision", &base, &count.to_string()]);
                    while out.iter().any(|row| row.id == id) {
                        count += 1;
                        id = stable_id(item.provider, &["collision", &base, &count.to_string()]);
                    }
                    let mut renamed = item;
                    renamed.id = id;
                    out.push(renamed);
                }
            }
        }
    }
    out
}

pub struct Published {
    pub thing: Thing,
    pub token: String,
    pub generation: u64,
}

pub struct Registry {
    secret: [u8; 32],
    generations: BTreeMap<String, u64>,
    providers: BTreeMap<String, Vec<Published>>,
}

impl Registry {
    pub fn new() -> Self {
        let mut secret = [0u8; 32];
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            let _ = file.read_exact(&mut secret);
        }
        Registry { secret, generations: BTreeMap::new(), providers: BTreeMap::new() }
    }

    fn sign(&self, raw: &[u8]) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("hmac key");
        mac.update(raw);
        mac.finalize().into_bytes().to_vec()
    }

    fn encode(&self, payload: &Value) -> String {
        // serde_json keeps object keys sorted, matching Python's sort_keys.
        let raw = serde_json::to_vec(payload).unwrap_or_default();
        format!("v1.{}.{}", URL_SAFE_NO_PAD.encode(&raw), URL_SAFE_NO_PAD.encode(self.sign(&raw)))
    }

    pub fn decode(&self, item_id: &str, token: &str) -> Result<Value, String> {
        let mut parts = token.splitn(3, '.');
        let (prefix, body, signature) = (parts.next(), parts.next(), parts.next());
        let (Some("v1"), Some(body), Some(signature)) = (prefix, body, signature) else {
            return Err("malformed activation token".into());
        };
        let raw = URL_SAFE_NO_PAD.decode(body).map_err(|_| "malformed activation token")?;
        let supplied = URL_SAFE_NO_PAD.decode(signature).map_err(|_| "malformed activation token")?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("hmac key");
        mac.update(&raw);
        mac.verify_slice(&supplied).map_err(|_| "activation token authentication failed")?;
        let payload: Value = serde_json::from_slice(&raw).map_err(|_| "activation token payload is invalid")?;
        if payload.get("id").and_then(Value::as_str) != Some(item_id) {
            return Err("activation token does not belong to this item".into());
        }
        Ok(payload)
    }

    pub fn publish(&mut self, provider: &str, items: Vec<Thing>) -> Vec<Value> {
        let generation = self.generations.get(provider).copied().unwrap_or(0) + 1;
        self.generations.insert(provider.to_string(), generation);
        let mut published = Vec::new();
        let mut public = Vec::new();
        for thing in dedupe(items) {
            let token = self.encode(&json!({
                "provider": provider,
                "id": thing.id,
                "generation": generation,
                "activation": thing.activation,
            }));
            public.push(thing.public(&token));
            published.push(Published { thing, token, generation });
        }
        self.providers.insert(provider.to_string(), published);
        public
    }

    pub fn current(&self, provider: &str, item_id: &str) -> Option<&Published> {
        self.providers.get(provider)?.iter().find(|row| row.thing.id == item_id)
    }

    pub fn token_is_current(&self, payload: &Value) -> bool {
        let provider = payload.get("provider").and_then(Value::as_str).unwrap_or("");
        let id = payload.get("id").and_then(Value::as_str).unwrap_or("");
        self.current(provider, id)
            .map(|row| payload.get("generation").and_then(Value::as_u64) == Some(row.generation))
            .unwrap_or(false)
    }

    pub fn all_public(&self) -> Vec<Value> {
        self.providers.values().flatten().map(|row| row.thing.public(&row.token)).collect()
    }

    pub fn provider_public(&self, provider: &str) -> Vec<Value> {
        self.providers
            .get(provider)
            .map(|rows| rows.iter().map(|row| row.thing.public(&row.token)).collect())
            .unwrap_or_default()
    }

    pub fn provider_names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_matches_python_with_safe_chars() {
        assert_eq!(quote("a b/ü~-._"), "a%20b%2F%C3%BC~-._");
        assert_eq!(stable_id("p", &["x y", "1"]), "p:x%20y/1");
    }

    #[test]
    fn ui_uuid_is_stable_and_matches_python() {
        // uuid5(NS, '["Hyprland","window","hyprland:0x1/2/3"]') from the Python helper.
        assert_eq!(ui_uuid("Hyprland", "window", "hyprland:0x1/2/3"), ui_uuid("Hyprland", "window", "hyprland:0x1/2/3"));
        assert_eq!(ui_uuid("a", "b", "c").len(), 36);
    }

    #[test]
    fn tokens_authenticate_and_bind_to_the_item() {
        let mut registry = Registry::new();
        let mut thing = Thing::new("p:1".into(), "window", "Hyprland", "One");
        thing.activation = json!({"address": "0x1"});
        let public = registry.publish("hyprland", vec![thing]);
        let token = public[0]["activationToken"].as_str().unwrap().to_string();
        assert!(registry.decode("p:1", &token).is_ok());
        assert!(registry.decode("p:2", &token).is_err());
        assert!(registry.decode("p:1", &format!("{token}x")).is_err());
        assert!(registry.token_is_current(&registry.decode("p:1", &token).unwrap()));
        registry.publish("hyprland", vec![]);
        assert!(!registry.token_is_current(&registry.decode("p:1", &token).unwrap()));
    }

    #[test]
    fn colliding_ids_for_distinct_targets_get_a_suffix() {
        let mut a = Thing::new("p:1".into(), "window", "Hyprland", "A");
        a.activation = json!({"n": 1});
        let mut b = Thing::new("p:1".into(), "window", "Hyprland", "B");
        b.activation = json!({"n": 2});
        let rows = dedupe(vec![a.clone(), b, a]);
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].id, rows[1].id);
    }
}
