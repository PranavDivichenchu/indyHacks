//! Hivemind — an open marketplace where AI agents meet, discover each other,
//! and collaborate. Single self-contained service: registry, discovery,
//! routing, a server-side LLM brain for browser-native agents, a coordinator
//! that turns any goal into a team, and a live web UI.

mod hub;
mod models;

fn main() {
    println!("Hivemind — core hub in place.");
}

#[cfg(test)]
mod tests {
    use super::hub::Hub;
    use super::models::{Agent, CreateAgent, Message};

    fn make(name: &str, caps: &str) -> Agent {
        Agent::from_create(CreateAgent {
            name: name.to_string(),
            role: String::new(),
            capabilities: serde_json::json!(caps),
            persona: String::new(),
        })
    }

    #[test]
    fn discovery_ranks_and_excludes_self() {
        let hub = Hub::new();
        let lead = hub.add_agent(make("Lead", "planning,coordination"));
        hub.add_agent(make("Foodie", "restaurants,dining,menus"));
        hub.add_agent(make("Scheduler", "calendar,availability"));

        let matches = hub.discover("who knows restaurants and dining", Some(&lead.id));
        assert_eq!(matches.first().map(|a| a.name.as_str()), Some("Foodie"));
        assert!(matches.iter().all(|a| a.id != lead.id), "must exclude the asker");
    }

    #[test]
    fn routing_drains_inbox_once() {
        let hub = Hub::new();
        let a = hub.add_agent(make("A", "x"));
        let b = hub.add_agent(make("B", "y"));
        hub.route(Message {
            id: "m1".into(),
            from_id: a.id.clone(),
            to_id: b.id.clone(),
            kind: "request".into(),
            content: "hi".into(),
            task_id: None,
            ts: 0.0,
        });
        assert_eq!(hub.drain_inbox(&b.id).len(), 1);
        assert_eq!(hub.drain_inbox(&b.id).len(), 0, "inbox should drain");
    }

    #[test]
    fn feed_records_events() {
        let hub = Hub::new();
        hub.add_agent(make("A", "x"));
        let types: Vec<String> = hub.history().into_iter().map(|e| e.event_type).collect();
        assert!(types.contains(&"agent_joined".to_string()));
    }
}
