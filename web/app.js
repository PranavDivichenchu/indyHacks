// app.js — live wiring for the Hivemind marketplace, rendered as a tty console.
//
// Same server contract as before (/api/*, SSE on /api/events). The renderer is
// terminal-styled: every event becomes a timestamped logline with a glyph and
// tree-branch structure, agents show up as a `ps`-style process list, and the
// coordinator's assembled answer prints into the result pane.

const $ = (id) => document.getElementById(id);
const feedEl    = $("feed");
const rosterEl  = $("roster");
const resultEl  = $("result");
const statusEl  = $("status");
const llmEl     = $("llm");
const countEl   = $("agentCount");
const capHintEl = $("capHint");
const clockEl   = $("clock");
const slAgents  = $("sl-agents");

const agents = new Map(); // id -> agent
let booted = false;       // whether we've cleared the placeholder logline

function esc(s) {
  return String(s ?? "").replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
}

// HH:MM:SS for the logline gutter + the title-bar clock.
function stamp(d = new Date()) {
  const p = (n) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

// Render a list of capability strings as ‹token› chips.
function toks(list) {
  return (list || []).map((c) => `<span class="tok">${esc(c)}</span>`).join("");
}

// ---------------- roster (process list) ----------------
function renderRoster() {
  rosterEl.innerHTML = "";
  const list = [...agents.values()].sort((a, b) => a.created_at - b.created_at);
  countEl.textContent = `[${list.length}]`;
  slAgents.textContent = `${list.length} proc${list.length === 1 ? "" : "s"}`;

  for (const a of list) {
    const el = document.createElement("div");
    el.className = "proc" + (a.kind === "coordinator" ? " coordinator" : "");
    const pid = a.id.replace(/^agent_/, "");
    const kill = a.kind === "coordinator"
      ? ""
      : `<button class="kill" title="kill" data-id="${esc(a.id)}">[kill]</button>`;
    el.innerHTML =
      `<span class="pname">${esc(a.name)}</span> ` +
      `<span class="pid">#${esc(pid)}</span>${kill}` +
      `<div class="prole">${esc(a.role)}</div>` +
      `<div class="pcaps">${a.capabilities.map((c) => `<b>${esc(c)}</b>`).join(" ")}</div>`;
    rosterEl.appendChild(el);
  }

  // capability hint under the goal box
  const caps = [...new Set(list.flatMap((a) => a.capabilities))].sort();
  capHintEl.innerHTML = caps.length
    ? `online: ${caps.map((c) => `<b>${esc(c)}</b>`).join(" · ")}`
    : `// no agents online — spawn one so the hive has someone to recruit.`;
}

rosterEl.addEventListener("click", async (e) => {
  const btn = e.target.closest(".kill");
  if (!btn) return;
  await fetch(`/api/agents/${btn.dataset.id}`, { method: "DELETE" });
});

// ---------------- feed (the log) ----------------
function log(type, glyph, html) {
  if (!booted) { feedEl.innerHTML = ""; booted = true; }
  const el = document.createElement("div");
  el.className = `logline ${type}`;
  el.innerHTML = `<span class="ts">${stamp()}</span><span class="glyph">${glyph}</span>${html}`;
  feedEl.appendChild(el);
  feedEl.scrollTop = feedEl.scrollHeight;
  while (feedEl.children.length > 220) feedEl.removeChild(feedEl.firstChild);
}

function handle(ev) {
  const d = ev.data || {};
  switch (ev.type) {
    case "agent_joined": {
      const a = d.agent;
      agents.set(a.id, a);
      renderRoster();
      if (a.kind !== "coordinator") {
        log("agent_joined", "+", `<span class="who">${esc(a.name)}</span> <span class="body">spawned · offers</span> ${toks(a.capabilities)}`);
      }
      break;
    }
    case "agent_left": {
      agents.delete(d.id);
      renderRoster();
      log("agent_left", "-", `<span class="who">${esc(d.name)}</span> <span class="body">exited</span>`);
      break;
    }
    case "goal_start": {
      resultEl.className = "result working";
      resultEl.textContent = "hive working… ";
      log("goal_start", "▶", `<span class="tag">goal</span> hive.run(<span class="body">"${esc(d.goal)}"</span>)`);
      break;
    }
    case "plan": {
      log("plan", "├", `<span class="who">${esc(d.by || "coordinator")}</span> <span class="body">decompose →</span> ${toks(d.needs) || "<span class='body'>—</span>"}`);
      break;
    }
    case "search": {
      const found = (d.matches || []).map((m) => m.name);
      const tail = found.length
        ? `<span class="body">found</span> ${found.map((n) => `<span class="who">${esc(n)}</span>`).join(", ")}`
        : `<span class="body">no match</span>`;
      log("search", "│ ⌕", `<span class="body">grep</span> <em>${esc(d.query)}</em> .... ${tail}`);
      break;
    }
    case "message": {
      const m = d.message;
      const reply = m.kind === "reply";
      log("message" + (reply ? " reply" : ""), "│",
        `<span class="who">${esc(d.from_name)}</span>` +
        `<span class="arrow"> ${reply ? "◂──" : "──▸"} </span>` +
        `<span class="who">${esc(d.to_name)}</span> ` +
        `<span class="body">${esc(m.content)}</span>`);
      break;
    }
    case "goal_done": {
      resultEl.className = "result ready";
      resultEl.textContent = d.result || "(no result)";
      const hired = (d.hired || []).join(", ") || "no one";
      const unmet = (d.unmet || []).length ? ` · unmet: ${(d.unmet).map(esc).join(", ")}` : "";
      log("goal_done", "└", `<span class="who">${esc(d.by || "coordinator")}</span> <span class="body">assembled · exit 0 · hired</span> ${esc(hired)}${unmet}`);
      break;
    }
  }
}

// ---------------- spawn agent ----------------
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
    msg.textContent = `${name} live on marketplace`;
    $("ag-name").value = "";
    $("ag-caps").value = "";
    $("ag-persona").value = "";
  } else {
    const err = await res.json().catch(() => ({}));
    msg.className = "form-msg err";
    msg.textContent = err.error || "spawn failed";
  }
  setTimeout(() => (msg.textContent = ""), 4000);
});

// ---------------- dispatch a goal ----------------
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
      llmEl.textContent = "llm:live";
      llmEl.className = "led llm";
    } else {
      llmEl.textContent = "llm:demo";
      llmEl.className = "led";
    }
  } catch (_) {}
}

function connect() {
  const src = new EventSource("/api/events");
  src.onopen  = () => { statusEl.textContent = "online"; statusEl.className = "led on"; };
  src.onerror = () => { statusEl.textContent = "reconn…"; statusEl.className = "led err"; };
  src.onmessage = (e) => { try { handle(JSON.parse(e.data)); } catch (_) {} };
}

// tick the title-bar clock
function tickClock() { clockEl.textContent = stamp(); }
tickClock();
setInterval(tickClock, 1000);

loadHealth();
connect();
