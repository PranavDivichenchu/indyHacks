//! Hivemind — an open marketplace where AI agents meet, discover each other,
//! and collaborate. A single self-contained service: registry, discovery,
//! routing, a server-side LLM brain for browser-native agents, a coordinator
//! that turns any goal into a team, and a live web UI.
//!
//! HTTP/SSE API (browser ⇄ server):
//!   POST   /api/agents            create a browser-native agent
//!   GET    /api/agents            list everyone on the marketplace
//!   DELETE /api/agents/:id        remove an agent
//!   GET    /api/capabilities      distinct capabilities currently offered
//!   POST   /api/goal              hand the coordinator a goal (runs async)
//!   GET    /api/events            Server-Sent Events stream (the live feed)
//!   GET    /api/health            liveness + whether the LLM is wired up
//!   GET    /*                     the web UI (static files)

mod brain;
mod coordinator;
mod hub;
mod models;

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use brain::Brain;
use hub::Hub;
use models::{Agent, CreateAgent, GoalBody};

#[derive(Clone)]
struct App {
    hub: Hub,
    brain: Brain,
}

/// Load KEY=VALUE lines from `.env` in the project root (if present). Does not
/// override variables already set in the environment.
fn load_dotenv() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        if !key.is_empty() && std::env::var(key).is_err() {
            // SAFETY: called before any threads are spawned.
            unsafe { std::env::set_var(key, val) };
        }
    }
}

#[tokio::main]
async fn main() {
    load_dotenv();
    tracing_subscriber::fmt().with_target(false).init();

    let app_state = App {
        hub: Hub::new(),
        brain: Brain::new(),
    };

    // The coordinator is a permanent marketplace resident that recruits others.
    let coordinator = app_state.hub.add_agent(Agent {
        id: models::short_id("agent"),
        name: "Coordinator".to_string(),
        role: "Recruits agents from the marketplace to accomplish any goal".to_string(),
        capabilities: vec!["planning".into(), "coordination".into(), "delegation".into()],
        persona: "You are the Coordinator. You break goals into needs, recruit \
                  specialists, and synthesize their answers."
            .to_string(),
        kind: "coordinator".to_string(),
        created_at: models::now(),
        last_seen: models::now(),
    });
    let coordinator_id = coordinator.id.clone();

    let web_dir = std::env::var("HIVEMIND_WEB_DIR").unwrap_or_else(|_| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("web")
            .to_string_lossy()
            .into_owned()
    });

    let api = Router::new()
        .route("/agents", post(create_agent).get(list_agents))
        .route("/agents/:id", axum::routing::delete(remove_agent))
        .route("/capabilities", get(capabilities))
        .route("/goal", post(post_goal))
        .route("/events", get(events))
        .route("/health", get(health))
        .with_state((app_state.clone(), coordinator_id));

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(&web_dir).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive());

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Hivemind listening on http://{addr}  (LLM: {})",
        if app_state.brain.is_live() { "live" } else { "demo/mock" });

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

type Ctx = State<(App, String)>;

async fn create_agent(State((app, _coord)): Ctx, Json(body): Json<CreateAgent>) -> impl IntoResponse {
    let name = body.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "name is required" }))).into_response();
    }
    if models::normalize_caps(body.capabilities.clone()).is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "at least one capability is required" }))).into_response();
    }
    let agent = app.hub.add_agent(Agent::from_create(body));
    (StatusCode::CREATED, Json(agent)).into_response()
}

async fn list_agents(State((app, _)): Ctx) -> Json<Vec<Agent>> {
    Json(app.hub.agents())
}

async fn remove_agent(State((app, coord)): Ctx, Path(id): Path<String>) -> impl IntoResponse {
    if id == coord {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "the coordinator cannot be removed" })));
    }
    let ok = app.hub.remove_agent(&id);
    (StatusCode::OK, Json(serde_json::json!({ "removed": ok })))
}

async fn capabilities(State((app, _)): Ctx) -> Json<Vec<String>> {
    Json(app.hub.capability_index())
}

/// Accept a goal and run it in the background so the request returns instantly;
/// the whole collaboration streams to /api/events as it happens.
async fn post_goal(State((app, coord)): Ctx, Json(body): Json<GoalBody>) -> impl IntoResponse {
    let goal = body.goal.trim().to_string();
    if goal.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "goal is required" })));
    }
    tokio::spawn(async move {
        coordinator::run_goal(&app.hub, &app.brain, &coord, &goal).await;
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "ok": true })))
}

async fn health(State((app, _)): Ctx) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "llm": if app.brain.is_live() { "live" } else { "mock" },
        "agents": app.hub.agents().len(),
    }))
}

/// SSE feed: replay recent history, then stream live events.
async fn events(State((app, _)): Ctx) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let history = app.hub.history();
    let mut rx = app.hub.subscribe();
    let stream = async_stream::stream! {
        for ev in history {
            if let Ok(sse) = SseEvent::default().json_data(&ev) {
                yield Ok(sse);
            }
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Ok(sse) = SseEvent::default().json_data(&ev) {
                        yield Ok(sse);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
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

    #[tokio::test]
    async fn brain_mock_is_named_and_safe() {
        let b = super::brain::Brain::new();
        if !b.is_live() {
            let out = b.think("You are Foodie, a restaurant expert.", "Suggest a venue.", 200).await;
            assert!(out.contains("Foodie"), "mock reply should be flavored by speaker: {out}");
            assert!(!out.is_empty());
        }
    }

    #[tokio::test]
    async fn coordinator_recruits_from_the_marketplace() {
        let hub = Hub::new();
        let brain = super::brain::Brain::new();
        let coord = hub.add_agent(make("Coordinator", "planning,coordination"));
        hub.add_agent(make("Backend", "endpoint,server,api"));
        hub.add_agent(make("Tester", "testing,qa"));
        hub.add_agent(make("Docs", "docs,documentation"));

        let result = super::coordinator::run_goal(
            &hub,
            &brain,
            &coord.id,
            "Build an endpoint, add testing, and write docs",
        )
        .await;
        assert!(!result.is_empty());

        let hired: Vec<String> = hub.history().into_iter()
            .filter(|e| e.event_type == "message")
            .filter_map(|e| {
                let m = e.data.get("message")?;
                (m.get("kind")?.as_str()? == "request")
                    .then(|| e.data.get("to_name")?.as_str().map(|s| s.to_string())).flatten()
            }).collect();
        for who in ["Backend", "Tester", "Docs"] {
            assert!(hired.contains(&who.to_string()), "expected to hire {who}, hired {hired:?}");
        }
    }

    fn make_coordinator(caps: &str) -> Agent {
        let mut a = make("Coordinator", caps);
        a.kind = "coordinator".to_string();
        a
    }

    #[tokio::test]
    async fn coordinator_recruits_specialists_not_meta_capabilities() {
        let hub = Hub::new();
        let brain = super::brain::Brain::new();
        let coord = hub.add_agent(make_coordinator("planning,coordination"));
        hub.add_agent(make("Foodie", "restaurants,menus,dining"));
        hub.add_agent(make("Scheduler", "calendar,availability"));
        hub.add_agent(make("Gifts", "gifts,wishlist"));

        let result = super::coordinator::run_goal(
            &hub,
            &brain,
            &coord.id,
            "plan a surprise birthday dinner",
        )
        .await;

        assert!(
            !result.contains("Couldn't recruit anyone"),
            "should hire specialists, got: {result}"
        );
        let hired: Vec<String> = hub
            .history()
            .into_iter()
            .filter(|e| e.event_type == "message")
            .filter_map(|e| {
                let m = e.data.get("message")?;
                (m.get("kind")?.as_str()? == "request")
                    .then(|| e.data.get("to_name")?.as_str().map(|s| s.to_string()))
                    .flatten()
            })
            .collect();
        assert!(
            hired.iter().any(|n| n == "Foodie"),
            "expected Foodie, hired {hired:?}"
        );
    }
}
