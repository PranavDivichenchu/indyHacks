//! models.rs — the shared vocabulary of the Hivemind marketplace.
//!
//! Every shape that crosses the wire (browser ⇄ server) lives here as a serde
//! struct, so the API, the agent runtime, and the web UI all agree on what an
//! agent, a message, and a feed event look like.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Short, readable, unique id like `agent_3f2a9c`.
pub fn short_id(prefix: &str) -> String {
    let hex = Uuid::new_v4().simple().to_string();
    format!("{}_{}", prefix, &hex[..6])
}

/// Seconds since the Unix epoch (float), used for timestamps.
pub fn now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// A browser-native agent living on the marketplace. `persona` is the system
/// instruction the server LLM uses to answer *as* this agent, so anyone can
/// stand up a real, useful agent from a web form with zero code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub role: String,
    pub capabilities: Vec<String>,
    /// System instruction that defines how this agent thinks/answers.
    pub persona: String,
    /// "human" agents are created by visitors; "coordinator" is the recruiter.
    pub kind: String,
    pub created_at: f64,
    /// Last time we saw activity from/about this agent (for liveness in the UI).
    pub last_seen: f64,
}

/// Body the browser POSTs to create an agent on the marketplace.
#[derive(Debug, Deserialize)]
pub struct CreateAgent {
    pub name: String,
    #[serde(default)]
    pub role: String,
    /// Accepts a list of strings or a comma-separated string.
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub persona: String,
}

impl Agent {
    pub fn from_create(c: CreateAgent) -> Self {
        let capabilities = normalize_caps(c.capabilities);
        let role = if c.role.trim().is_empty() {
            format!("Specializes in {}", capabilities.join(", "))
        } else {
            c.role.trim().to_string()
        };
        let persona = if c.persona.trim().is_empty() {
            format!(
                "You are {}, an agent on an open marketplace. Your role: {}. \
                 You specialize in: {}. When a peer asks you something in your \
                 specialty, give a concrete, useful answer in 1-3 sentences.",
                c.name.trim(),
                role,
                capabilities.join(", ")
            )
        } else {
            c.persona.trim().to_string()
        };
        let t = now();
        Agent {
            id: short_id("agent"),
            name: c.name.trim().to_string(),
            role,
            capabilities,
            persona,
            kind: "human".to_string(),
            created_at: t,
            last_seen: t,
        }
    }
}

/// Normalize capabilities sent as either `["a","b"]` or `"a, b"`.
pub fn normalize_caps(v: serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(a) => a
            .into_iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_lowercase()))
            .filter(|s| !s.is_empty())
            .collect(),
        serde_json::Value::String(s) => s
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect(),
        _ => vec![],
    }
}

/// A message routed between agents through the marketplace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub kind: String, // "request" | "reply"
    pub content: String,
    pub task_id: Option<String>,
    pub ts: f64,
}

/// The body the browser POSTs to give a goal to a coordinator.
#[derive(Debug, Deserialize)]
pub struct GoalBody {
    #[serde(default)]
    pub goal: String,
}

/// A typed entry on the live feed the UI subscribes to. `data`'s shape depends
/// on `event_type`; the UI keys off the type.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub ts: f64,
    pub data: serde_json::Value,
}

impl Event {
    pub fn new(event_type: &str, data: serde_json::Value) -> Self {
        Event {
            id: short_id("evt"),
            event_type: event_type.to_string(),
            ts: now(),
            data,
        }
    }
}
