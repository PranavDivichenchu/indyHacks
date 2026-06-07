//! hub.rs — the marketplace itself (in-memory state).
//!
//! The hub is the meeting place: it tracks every agent on the marketplace,
//! matches agents by capability (discovery), routes messages into per-agent
//! inboxes, and broadcasts every happening as an Event to all live subscribers
//! (the web UI). It is intentionally simple and self-contained: an
//! `Arc<Mutex<…>>` of state plus a tokio broadcast channel for the feed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::models::{now, Agent, Event, Message};

#[derive(Clone)]
pub struct Hub {
    state: Arc<Mutex<State>>,
    tx: broadcast::Sender<Event>,
}

struct State {
    agents: HashMap<String, Agent>,
    inboxes: HashMap<String, Vec<Message>>,
    history: Vec<Event>,
}

/// Cap how much feed history we replay to a fresh subscriber, so a long-lived
/// public deployment doesn't hand every new tab a giant backlog.
const MAX_HISTORY: usize = 500;

impl Hub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Hub {
            state: Arc::new(Mutex::new(State {
                agents: HashMap::new(),
                inboxes: HashMap::new(),
                history: Vec::new(),
            })),
            tx,
        }
    }

    // ------------------------------ registry ----------------------------- //
    pub fn add_agent(&self, agent: Agent) -> Agent {
        {
            let mut st = self.state.lock().unwrap();
            st.agents.insert(agent.id.clone(), agent.clone());
        }
        self.emit(Event::new("agent_joined", serde_json::json!({ "agent": agent })));
        agent
    }

    pub fn remove_agent(&self, id: &str) -> bool {
        let removed = {
            let mut st = self.state.lock().unwrap();
            st.inboxes.remove(id);
            st.agents.remove(id)
        };
        if let Some(a) = removed {
            self.emit(Event::new(
                "agent_left",
                serde_json::json!({ "id": a.id, "name": a.name }),
            ));
            true
        } else {
            false
        }
    }

    pub fn agents(&self) -> Vec<Agent> {
        let mut v: Vec<Agent> = self.state.lock().unwrap().agents.values().cloned().collect();
        v.sort_by(|a, b| a.created_at.partial_cmp(&b.created_at).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    pub fn get(&self, id: &str) -> Option<Agent> {
        self.state.lock().unwrap().agents.get(id).cloned()
    }

    pub fn name_of(&self, id: &str) -> String {
        self.get(id).map(|a| a.name).unwrap_or_else(|| id.to_string())
    }

    pub fn touch(&self, id: &str) {
        if let Some(a) = self.state.lock().unwrap().agents.get_mut(id) {
            a.last_seen = now();
        }
    }

    // ----------------------------- discovery ----------------------------- //
    /// Find agents whose capabilities/name/role overlap a free-text query,
    /// ranked by overlap. Forgiving by design so a coordinator can search in
    /// natural language. Excludes the asker so it never finds itself.
    pub fn discover(&self, query: &str, exclude_id: Option<&str>) -> Vec<Agent> {
        let words = tokenize(query);
        let matches: Vec<Agent> = {
            let st = self.state.lock().unwrap();
            let mut scored: Vec<(usize, Agent)> = st
                .agents
                .values()
                .filter(|a| exclude_id != Some(a.id.as_str()))
                .filter_map(|a| {
                    let hay = tokenize(&format!(
                        "{} {} {}",
                        a.capabilities.join(" "),
                        a.name,
                        a.role
                    ));
                    let score = words.iter().filter(|w| hay.contains(*w)).count();
                    (score > 0).then(|| (score, a.clone()))
                })
                .collect();
            scored.sort_by(|x, y| y.0.cmp(&x.0));
            scored.into_iter().map(|(_, a)| a).collect()
        };
        let by_name = exclude_id.map(|id| self.name_of(id));
        self.emit(Event::new(
            "search",
            serde_json::json!({
                "query": query,
                "by": exclude_id,
                "by_name": by_name,
                "matches": matches,
            }),
        ));
        matches
    }

    /// Every distinct capability currently on the marketplace (for UI / hints).
    pub fn capability_index(&self) -> Vec<String> {
        let st = self.state.lock().unwrap();
        let mut set: Vec<String> = vec![];
        for a in st.agents.values() {
            for c in &a.capabilities {
                if !set.contains(c) {
                    set.push(c.clone());
                }
            }
        }
        set.sort();
        set
    }

    // ------------------------------ routing ------------------------------ //
    pub fn route(&self, msg: Message) -> Message {
        let from_name = self.name_of(&msg.from_id);
        let to_name = self.name_of(&msg.to_id);
        {
            let mut st = self.state.lock().unwrap();
            st.inboxes.entry(msg.to_id.clone()).or_default().push(msg.clone());
        }
        self.touch(&msg.from_id);
        self.emit(Event::new(
            "message",
            serde_json::json!({
                "message": msg,
                "from_name": from_name,
                "to_name": to_name,
            }),
        ));
        msg
    }

    /// Return and clear an agent's pending messages (poll-and-drain).
    pub fn drain_inbox(&self, id: &str) -> Vec<Message> {
        let mut st = self.state.lock().unwrap();
        std::mem::take(st.inboxes.entry(id.to_string()).or_default())
    }

    // ------------------------------- feed -------------------------------- //
    pub fn emit(&self, ev: Event) {
        {
            let mut st = self.state.lock().unwrap();
            st.history.push(ev.clone());
            if st.history.len() > MAX_HISTORY {
                let cut = st.history.len() - MAX_HISTORY;
                st.history.drain(0..cut);
            }
        }
        let _ = self.tx.send(ev); // ok if no subscribers yet
    }

    pub fn history(&self) -> Vec<Event> {
        self.state.lock().unwrap().history.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

fn tokenize(text: &str) -> std::collections::HashSet<String> {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|s| s.len() > 1)
        .map(|s| s.to_string())
        .collect()
}
