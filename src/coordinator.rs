//! coordinator.rs — turns any goal into a team, server-side.
//!
//! The coordinator is the recruiter of the marketplace. Hand it any goal and it:
//!   1. DECOMPOSE — break the goal into the capabilities it needs (Gemini when
//!      available; otherwise match against what's actually on the marketplace).
//!   2. SEARCH    — for each capability, discover an agent that has it.
//!   3. DELEGATE  — ask each found agent the right question; the server's brain
//!      answers on that agent's behalf (browser-native agents have no process).
//!   4. ASSEMBLE  — synthesize the answers into one result.
//!
//! It knows nothing about any specific use case — whatever agents are online and
//! whatever the goal is, it figures out who to talk to at runtime.

use crate::brain::Brain;
use crate::hub::Hub;
use crate::models::{now, short_id, Event, Message};

/// One decomposed need: a searchable capability + the question to ask whoever
/// provides it.
struct Need {
    capability: String,
    question: String,
}

/// Run a full goal: decompose -> search -> delegate -> assemble. Emits feed
/// events throughout so the UI tells the story live. Returns the final result.
pub async fn run_goal(hub: &Hub, brain: &Brain, coordinator_id: &str, goal: &str) -> String {
    let coord_name = hub.name_of(coordinator_id);
    hub.emit(Event::new(
        "goal_start",
        serde_json::json!({ "goal": goal, "by": coord_name }),
    ));

    // 1. DECOMPOSE.
    let needs = decompose(hub, brain, goal).await;
    hub.emit(Event::new(
        "plan",
        serde_json::json!({
            "by": coord_name,
            "goal": goal,
            "needs": needs.iter().map(|n| n.capability.clone()).collect::<Vec<_>>(),
        }),
    ));

    // 2+3. SEARCH + DELEGATE. The server brain answers as each hired agent.
    let mut parts: Vec<(String, String, String)> = vec![]; // (agent, capability, answer)
    let mut unmet: Vec<String> = vec![];
    for need in &needs {
        let matches = hub.discover(&need.capability, Some(coordinator_id));
        let Some(agent) = matches.into_iter().next() else {
            unmet.push(need.capability.clone());
            continue;
        };

        let task_id = short_id("task");
        // The coordinator's request, on the feed (agent talking to agent).
        hub.route(Message {
            id: short_id("msg"),
            from_id: coordinator_id.to_string(),
            to_id: agent.id.clone(),
            kind: "request".into(),
            content: need.question.clone(),
            task_id: Some(task_id.clone()),
            ts: now(),
        });

        // The hired agent "answers": server brain thinks in that agent's voice.
        let answer = brain.think(&agent.persona, &need.question, 300).await;
        hub.touch(&agent.id);
        hub.route(Message {
            id: short_id("msg"),
            from_id: agent.id.clone(),
            to_id: coordinator_id.to_string(),
            kind: "reply".into(),
            content: answer.clone(),
            task_id: Some(task_id),
            ts: now(),
        });

        parts.push((agent.name.clone(), need.capability.clone(), answer));
    }

    // 4. ASSEMBLE.
    let result = assemble(brain, goal, &parts, &unmet).await;
    hub.emit(Event::new(
        "goal_done",
        serde_json::json!({
            "by": coord_name,
            "goal": goal,
            "result": result,
            "unmet": unmet,
            "hired": parts.iter().map(|(n, _, _)| n.clone()).collect::<Vec<_>>(),
        }),
    ));
    result
}

/// Break a goal into needed capabilities. Uses the LLM when available; otherwise
/// matches the goal against capabilities actually offered on the marketplace, so
/// it still recruits sensibly with no API key.
async fn decompose(hub: &Hub, brain: &Brain, goal: &str) -> Vec<Need> {
    if brain.is_live() {
        let system = "You decompose a goal into the specialist capabilities needed to \
                      accomplish it. Reply with ONLY a JSON array of objects \
                      [{\"capability\":\"<short keyword>\",\"question\":\"<one question>\"}]. \
                      Capabilities must be short searchable keywords (e.g. \"restaurants\", \
                      \"unit testing\"), not sentences. 2-4 items.";
        let prompt = format!("Goal: {goal}");
        let raw = brain.think(system, &prompt, 500).await;
        if let Some(needs) = parse_needs(&raw) {
            if !needs.is_empty() {
                return needs;
            }
        }
        // fall through to market-aware matching if parsing failed
    }
    decompose_from_market(hub, goal)
}

/// Brain-free decomposition: pick capabilities already on the marketplace that
/// overlap the goal's words. Models "shopping the marketplace for what's there".
fn decompose_from_market(hub: &Hub, goal: &str) -> Vec<Need> {
    let goal_words = words(goal);
    let caps = hub.capability_index();
    if caps.is_empty() {
        let first = goal.split_whitespace().next().unwrap_or("help").to_lowercase();
        return vec![Need {
            capability: first,
            question: goal.to_string(),
        }];
    }
    let mut scored: Vec<(usize, String)> = caps
        .into_iter()
        .map(|c| (words(&c).intersection(&goal_words).count(), c))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let overlapping: Vec<String> = scored.iter().filter(|(s, _)| *s > 0).map(|(_, c)| c.clone()).collect();
    let chosen = if overlapping.is_empty() {
        scored.into_iter().map(|(_, c)| c).collect::<Vec<_>>()
    } else {
        overlapping
    };
    chosen
        .into_iter()
        .take(4)
        .map(|c| Need {
            question: format!("For the goal '{goal}', help with the '{c}' part."),
            capability: c,
        })
        .collect()
}

/// Synthesize the hired agents' answers into one result.
async fn assemble(brain: &Brain, goal: &str, parts: &[(String, String, String)], unmet: &[String]) -> String {
    if parts.is_empty() {
        let miss = if unmet.is_empty() {
            String::new()
        } else {
            format!(" No agents on the marketplace offer: {}.", unmet.join(", "))
        };
        return format!("Couldn't recruit anyone for \"{goal}\".{miss}");
    }
    let bullets: String = parts
        .iter()
        .map(|(name, cap, ans)| format!("- {name} (for {cap}): {ans}"))
        .collect::<Vec<_>>()
        .join("\n");
    let note = if unmet.is_empty() {
        String::new()
    } else {
        format!("\n\nStill unmet (no provider on the marketplace): {}", unmet.join(", "))
    };
    let system = "You synthesize specialist agents' answers into one clear, friendly result. \
                  Be concise (4-6 short lines) and credit each agent's contribution naturally.";
    let prompt = format!("Goal: {goal}\n\nRecruited agents replied:\n{bullets}{note}");
    brain.think(system, &prompt, 700).await
}

// ----------------------------- helpers ----------------------------------- //

fn words(text: &str) -> std::collections::HashSet<String> {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Parse the LLM's JSON array of {capability, question} (tolerates code fences).
fn parse_needs(text: &str) -> Option<Vec<Need>> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    let arr: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let items = arr.as_array()?;
    let mut needs = vec![];
    for it in items {
        let cap = it.get("capability").and_then(|v| v.as_str()).unwrap_or("").trim();
        let q = it.get("question").and_then(|v| v.as_str()).unwrap_or("").trim();
        if !cap.is_empty() {
            needs.push(Need {
                capability: cap.to_lowercase(),
                question: if q.is_empty() {
                    format!("Help with '{cap}'.")
                } else {
                    q.to_string()
                },
            });
        }
    }
    Some(needs)
}
