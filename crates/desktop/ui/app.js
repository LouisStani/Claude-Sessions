const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const ELSEWHERE = "runs in another terminal: it cannot be opened here";

const terminal = new Terminal({
  fontFamily: "monospace",
  fontSize: 14,
  theme: { background: "#111111" },
});
const fit = new FitAddon.FitAddon();

// The session on the right: `output` tells its bytes apart from those of the session shown
// before, `generation` tells its end apart.
let current = null;
// A session can end before `open_session` has returned its generation.
let lastEnded = null;
let lastView = { loaded: false, sessions: [], errors: [] };

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function sessionItem(session) {
  const item = element("li", "session");
  const title = element("div", "title");
  title.append(element("span", "number", session.number ? `${session.number}` : ""));
  title.append(element("span", "name", session.title));
  if (session.unread > 0) title.append(element("span", "badge", `${session.unread} msg`));
  const detail = [session.account, session.state];
  if (session.isolated) detail.push("isolated");
  item.append(title, element("div", "detail", detail.join(" · ")));
  if (session.short_id) {
    item.classList.add("openable");
    if (current && current.shortId === session.short_id) item.classList.add("open");
    item.addEventListener("click", () => openSession(session));
  } else {
    item.title = ELSEWHERE;
  }
  return item;
}

function render(view) {
  lastView = view;
  const list = document.getElementById("sessions");
  if (!view.loaded) {
    list.replaceChildren(element("li", "muted", "loading…"));
  } else if (view.sessions.length === 0) {
    list.replaceChildren(element("li", "muted", "no sessions"));
  } else {
    list.replaceChildren(...view.sessions.map(sessionItem));
  }
  document.getElementById("errors").replaceChildren(...view.errors.map((error) => element("li", "error", error)));
}

function showEnd(session) {
  session.ended = true;
  terminal.write("\r\n\x1b[2m[session closed]\x1b[0m\r\n");
}

async function openSession(session) {
  if (current && current.shortId === session.short_id && !current.ended) {
    terminal.focus();
    return;
  }
  const output = new Channel();
  const opening = { shortId: session.short_id, output, generation: null, ended: false };
  current = opening;
  output.onmessage = (bytes) => {
    if (current === opening) terminal.write(new Uint8Array(bytes));
  };
  document.getElementById("placeholder").hidden = true;
  terminal.reset();
  render(lastView);
  try {
    opening.generation = await invoke("open_session", {
      account: session.account,
      shortId: session.short_id,
      columns: terminal.cols,
      rows: terminal.rows,
      output,
    });
  } catch (error) {
    if (current !== opening) return;
    showEnd(opening);
    terminal.write(`\x1b[31m${error}\x1b[0m\r\n`);
    return;
  }
  if (current === opening && lastEnded === opening.generation) showEnd(opening);
  terminal.focus();
}

function forwardInput(data) {
  if (!current || current.ended) return;
  invoke("write_input", { data }).catch((error) => console.error(error));
}

function forwardSize({ cols, rows }) {
  if (!current || current.ended) return;
  invoke("resize_pane", { columns: cols, rows }).catch((error) => console.error(error));
}

function loadWebgl() {
  try {
    const webgl = new WebglAddon.WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    terminal.loadAddon(webgl);
  } catch (error) {
    console.warn("WebGL unavailable, using the DOM renderer:", error);
  }
}

const container = document.getElementById("terminal");
terminal.loadAddon(fit);
terminal.open(container);
loadWebgl();
terminal.onData(forwardInput);
terminal.onResize(forwardSize);
new ResizeObserver(() => fit.fit()).observe(container);
fit.fit();

await listen("pane-ended", (event) => {
  lastEnded = event.payload;
  if (current && current.generation === event.payload) showEnd(current);
});
await listen("sessions", (event) => render(event.payload));
render(await invoke("current_list"));
