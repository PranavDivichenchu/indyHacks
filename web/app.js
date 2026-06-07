// app.js — live wiring for the Hivemind marketplace UI.
//
// Talks to the server API (/api/*) and renders everything in real time off the
// SSE feed: agents joining/leaving (left), the collaboration (center), and the
// coordinator's assembled result (right). Anyone can create an agent and give
// the hive a goal from here.

const $ = (id) => document.getElementById(id);
const feedEl = $("feed");
const rosterEl = $("roster");
const resultEl = $("result");
const statusEl = $("status");
const llmEl = $("llm");
const countEl = $("agentCount");
const capHintEl = $("capHint");

const agents = new Map(); // id -> agent

function esc(s) {
  return String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
}

// ---------------- roster ----------------
function renderRoster() {
  rosterEl.innerHTML = "";
  const list = [...agents.values()].sort((a, b) => a.created_at - b.created_at);
  countEl.textContent = list.length;
  for (const a of list) {
    const el = document.createElement("div");
    el.className = "agent" + (a.kind === "coordinator" ? " coordinator" : "");
    const badge = a.kind === "coordinator" ? `<span class="role-badge">coordinator</span>` : "";
    const rm = a.kind === "coordinator" ? "" : `<button class="rm" title="remove" data-id="${esc(a.id)}">✕</button>`;
    el.innerHTML = `
      <div class="top"><span class="name">${esc(a.name)}</span>${badge}${rm}</div>
      <div class="role">${esc(a.role)}</div>
      <div class="caps">${a.capabilities.map((c) => `<span class="cap">${esc(c)}</span>`).join("")}</div>`;
    rosterEl.appendChild(el);
  }
  // capability hint for the goal box
  const caps = [...new Set(list.flatMap((a) => a.capabilities))].sort();
  capHintEl.innerHTML = caps.length
    ? `Capabilities online: ${caps.map((c) => `<b>${esc(c)}</b>`).join(", ")}`
    : "No agents yet — add one on the left so the hive has someone to recruit.";
}

rosterEl.addEventListener("click", async (e) => {
  const btn = e.target.closest(".rm");
  if (!btn) return;
  await fetch(`/api/agents/${btn.dataset.id}`, { method: "DELETE" });
});

// ---------------- feed ----------------
function addRow(type, html) {
  const el = document.createElement("div");
  el.className = `row ${type}`;
  el.innerHTML = html;
  feedEl.appendChild(el);
  feedEl.scrollTop = feedEl.scrollHeight;
  // keep the feed from growing without bound in a long session
  while (feedEl.children.length > 200) feedEl.removeChild(feedEl.firstChild);
}

function handle(ev) {
  const d = ev.data || {};
  switch (ev.type) {
    case "agent_joined": {
      const a = d.agent;
      agents.set(a.id, a);
      renderRoster();
      if (a.kind !== "coordinator") {
        addRow("agent_joined", `<span class="tag">joined</span><span class="who">${esc(a.name)}</span>
          <span class="body">offers: ${a.capabilities.map((c) => `<span class="pill">${esc(c)}</span>`).join("")}</span>`);
      }
      break;
    }
    case "agent_left": {
      agents.delete(d.id);
      renderRoster();
      addRow("agent_left", `<span class="tag">left</span><span class="who">${esc(d.name)}</span> went offline`);
      break;
    }
    case "goal_start": {
      resultEl.className = "result working";
      resultEl.textContent = "The hive is working on it…";
      addRow("goal_start", `<span class="tag">goal</span><span class="body">🎯 ${esc(d.goal)}</span>`);
      break;
    }
    case "plan": {
      addRow("plan", `<span class="tag">decompose</span><span class="who">${esc(d.by)}</span> needs:
        <span class="body">${(d.needs || []).map((c) => `<span class="pill">${esc(c)}</span>`).join("") || "—"}</span>`);
      break;
    }
    case "search": {
      const found = (d.matches || []).map((m) => m.name);
      addRow("search", `<span class="tag">search</span>${esc(d.by_name || "someone")} looks for
        <em>"${esc(d.query)}"</em><span class="body">→ ${found.length ? found.map(esc).join(", ") : "no match"}</span>`);
      break;
    }
    case "message": {
      const m = d.message;
      const verb = m.kind === "reply" ? "replies to" : "asks";
      addRow("message", `<span class="who">${esc(d.from_name)}</span><span class="arrow">${verb} →</span>
        <span class="who">${esc(d.to_name)}</span><span class="body">${esc(m.content)}</span>`);
      break;
    }
    case "goal_done": {
      resultEl.className = "result ready";
      resultEl.textContent = d.result || "(no result)";
      const hired = (d.hired || []).join(", ") || "no one";
      addRow("goal_done", `<span class="tag">done</span><span class="who">${esc(d.by)}</span>
        assembled the result ✅<span class="body">recruited: ${esc(hired)}${(d.unmet || []).length ? ` · unmet: ${(d.unmet).map(esc).join(", ")}` : ""}</span>`);
      break;
    }
  }
}

// ---------------- create agent ----------------
$("createForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const name = $("ag-name").value.trim();
  const capabilities = $("ag-caps").value.trim();
  const persona = $("ag-persona").value.trim();
  const msg = $("createMsg");
  if (!name || !capabilities) return;
  const res = await fetch("/api/agents", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name, capabilities, persona }),
  });
  if (res.ok) {
    msg.className = "form-msg ok";
    msg.textContent = `${name} is live on the marketplace.`;
    $("ag-name").value = "";
    $("ag-caps").value = "";
    $("ag-persona").value = "";
  } else {
    const err = await res.json().catch(() => ({}));
    msg.className = "form-msg err";
    msg.textContent = err.error || "Could not add agent.";
  }
  setTimeout(() => (msg.textContent = ""), 4000);
});

// ---------------- give a goal ----------------
$("goalForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const goal = $("goal").value.trim();
  if (!goal) return;
  await fetch("/api/goal", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ goal }),
  });
});

// ---------------- bootstrap ----------------
async function loadHealth() {
  try {
    const h = await (await fetch("/api/health")).json();
    if (h.llm === "live") {
      llmEl.textContent = "⬡ LLM live";
      llmEl.className = "chip llm-live";
    } else {
      llmEl.textContent = "demo mode";
      llmEl.className = "chip";
    }
  } catch (_) {}
}

function connect() {
  const src = new EventSource("/api/events");
  src.onopen = () => { statusEl.textContent = "● live"; statusEl.className = "chip live"; };
  src.onerror = () => { statusEl.textContent = "reconnecting…"; statusEl.className = "chip"; };
  src.onmessage = (e) => {
    try { handle(JSON.parse(e.data)); } catch (_) {}
  };
}

loadHealth();
connect();
