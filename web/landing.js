// landing.js — health badge + animated terminal demo lines

const healthEl = document.getElementById("health");
const termDemo = document.getElementById("term-demo");
// Show live/demo status from the API.
fetch("/api/health")
  .then((r) => r.json())
  .then((data) => {
    const live = data?.llm === "live";
    healthEl.textContent = live ? "llm live" : "demo mode";
    healthEl.classList.add("live");
  })
  .catch(() => {
    healthEl.textContent = "offline";
  });

// Cycle a fresh logline in the terminal preview every few seconds.
const demoLines = [
  { ts: "14:03:01", cls: "search", g: "◎", text: '<span class="dim">search marketplace → matched Translator, Foodie</span>' },
  { ts: "14:03:04", cls: "", g: "→", text: '<span class="who">Translator</span> <span class="dim">"Menu translated — note: \'ris de veau\' is sweetbreads."</span>' },
  { ts: "14:03:08", cls: "done", g: "✓", text: '<span class="dim">result assembled — dinner plan + french menu</span>' },
  { ts: "14:04:12", cls: "plan", g: "◆", text: '<span class="dim">needs: ‹research› ‹writing› ‹review›</span>' },
  { ts: "14:04:14", cls: "search", g: "◎", text: '<span class="dim">search marketplace → matched Researcher, Writer</span>' },
];

let demoIdx = 0;

function appendDemoLine() {
  if (!termDemo) return;
  const line = demoLines[demoIdx % demoLines.length];
  demoIdx += 1;

  const el = document.createElement("div");
  el.className = `term-line ${line.cls}`.trim();
  el.innerHTML = `<span class="ts">${line.ts}</span><span class="g">${line.g}</span> ${line.text}`;
  termDemo.appendChild(el);

  // Keep the preview from growing forever.
  const lines = termDemo.querySelectorAll(".term-line");
  if (lines.length > 8) lines[0].remove();

  termDemo.scrollTop = termDemo.scrollHeight;
}

setInterval(appendDemoLine, 4200);
