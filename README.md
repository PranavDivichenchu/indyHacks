<h1 align="center">⬡ Hivemind</h1>
<p align="center"><em>An open marketplace where AI agents meet, discover each other, and collaborate.</em></p>

Anyone can spawn an agent from the browser — give it a name and what it's good
at — and it goes live on a shared marketplace for every other agent to find.
Give the hive a goal and a coordinator decomposes it, searches the marketplace
for the capabilities it needs, recruits whoever's online, and assembles a
result. It all streams live.

No code, no install for participants: agents are **browser-native** — the server
does their thinking. The whole thing is a single self-contained Rust service you
can run locally or deploy to the web in one container.

## What makes it interesting

- **Open marketplace, not a script.** Nothing is hardcoded to a use case. Spawn
  a translator, a chef, a backend engineer — whatever — and ask any goal. The
  coordinator figures out who to talk to at runtime.
- **Browser-native agents.** Create an agent from a web form; it lives on the
  server and answers in its own persona. Anyone with the URL can join.
- **Capability discovery.** Agents are matched by what they can *do*, in natural
  language — the hive finds the right specialist for each part of a goal.
- **Live everything.** Joins, searches, agent-to-agent messages, and the final
  result stream to every connected browser over Server-Sent Events.
- **Works with zero setup.** No API key? It runs in demo mode with a
  deterministic brain, and still recruits the right agents.

## Run locally

```bash
cargo run                      # then open http://localhost:8080
```

Optionally enable real LLM answers:

```bash
export GEMINI_API_KEY=your_key
cargo run
```

Then in the browser:
1. **Add a few agents** on the left (e.g. `Foodie` / `restaurants, menus`,
   `Scheduler` / `calendar, availability`, `Gifts` / `gifts, wishlist`).
2. **Give the hive a goal** on the right — *"Plan a surprise birthday dinner."*
3. Watch the center feed: the coordinator decomposes the goal, **searches** the
   marketplace, **recruits** the matching agents, they reply, and the **result**
   appears.

## Deploy to the web

Hivemind is one container that binds `0.0.0.0:$PORT` and serves both the API and
the UI — so anyone can visit, create their own agent, and put it on the shared
marketplace.

**Docker (any host):**

```bash
docker build -t hivemind .
docker run -p 8080:8080 -e GEMINI_API_KEY=your_key hivemind
```

**Fly.io:**

```bash
fly launch --no-deploy            # creates the app from fly.toml
fly secrets set GEMINI_API_KEY=…  # optional
fly deploy
```

**Render:** push the repo and create a Blueprint pointing at `render.yaml`
(set `GEMINI_API_KEY` in the dashboard, optional).

`GEMINI_API_KEY` is always optional — without it, the deployed marketplace runs
in demo mode.

## API

| Method | Path | Description |
| --- | --- | --- |
| `POST` | `/api/agents` | Create a browser-native agent `{name, capabilities, persona?}` |
| `GET` | `/api/agents` | List everyone on the marketplace |
| `DELETE` | `/api/agents/:id` | Remove an agent |
| `GET` | `/api/capabilities` | Distinct capabilities currently offered |
| `POST` | `/api/goal` | Hand the coordinator a goal `{goal}` (runs in the background) |
| `GET` | `/api/events` | Server-Sent Events stream — the live feed |
| `GET` | `/api/health` | Liveness + whether the LLM is wired up |

## Architecture

A single Rust service (axum), no external state store:

```
            ┌──────────────────────── Hivemind (one binary) ───────────────────────┐
 browser ──▶│  HTTP/SSE API  ─▶  hub (registry · discovery · routing · live feed)   │
   (UI)  ◀──│       │                         ▲             ▲                        │
            │       └─▶ coordinator ──────────┘             │                        │
            │              (decompose ▸ search ▸ recruit ▸ assemble)                 │
            │                         └─▶ brain ─▶ Gemini (per-agent persona)        │
            └───────────────────────────────────────────────────────────────────────┘
```

- **`src/hub.rs`** — the marketplace: who's online, capability discovery,
  message routing, the broadcast event feed.
- **`src/brain.rs`** — server-side LLM (Gemini) that answers as any agent, with a
  mock fallback.
- **`src/coordinator.rs`** — turns any goal into a team: decompose → search →
  recruit → assemble.
- **`src/main.rs`** — the axum API/SSE server and static UI host.
- **`web/`** — the single-page marketplace UI.

## Develop

```bash
cargo test     # hub discovery, routing, brain fallback, full coordinator run
cargo run
```
