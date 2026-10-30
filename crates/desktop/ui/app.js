import { Panes } from "./panes.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let lastView = { loaded: false, accounts: [], sessions: [], errors: [] };
const picker = { rows: [], selected: 0, query: 0 };
const status = { notice: "", timer: null };

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function elsewhere(session) {
  return `"${session.title}" runs in another terminal: it cannot be opened here`;
}

// A notice goes after 8 s. Errors of the list come first and stay as long as they last.
function setStatus(text) {
  clearTimeout(status.timer);
  status.notice = text;
  if (text) status.timer = setTimeout(() => setStatus(""), 8000);
  renderStatus();
}

// The right end of the bar: an error, a notice, or the name of the app.
function renderStatus() {
  const node = document.getElementById("status");
  const errors = lastView.errors.join(" · ");
  const text = errors || status.notice;
  node.className = errors ? "error" : text ? "notice" : "";
  node.title = text;
  if (text) node.replaceChildren(element("span", "status-text", text));
  else node.replaceChildren("claude-sessions", element("span", "cursor"));
}

// The state a chip shows: its color and the shape of its dot.
function chipState(session) {
  if (session.finished) return "finished";
  if (!session.short_id) return "elsewhere";
  return session.state;
}

function sessionItem(session) {
  const item = element("li", "session");
  const title = element("div", "title");
  title.append(element("span", "number", session.number ? `${session.number}` : ""));
  title.append(element("span", "name", session.title));
  if (session.unread > 0) title.append(element("span", "badge", `${session.unread} msg`));
  if (session.finished) title.append(element("span", "done", "done"));
  const detail = [session.account, session.state];
  if (session.isolated) detail.push("isolated");
  item.append(title, element("div", "detail", detail.join(" · ")));
  return item;
}

function chip(session) {
  const item = element("li", `chip ${chipState(session)}`);
  item.append(element("span", "pos", session.number ? `${session.number}` : ""));
  item.append(element("span", "dot", session.finished ? "✓" : ""));
  item.append(element("span", "chip-title", session.title));
  if (session.unread > 0) item.append(element("span", "badge", `${session.unread}`));
  if (panes.shows(session)) item.classList.add("open");
  item.addEventListener("click", () => openSession(session));
  item.addEventListener("mouseenter", () => showTip(session, item));
  item.addEventListener("mouseleave", hideTip);
  return item;
}

function showTip(session, item) {
  const tip = document.getElementById("tip");
  const state = [session.finished ? "done" : session.state];
  if (session.isolated) state.push("isolated");
  const head = element("div");
  head.append(element("b", "", session.title), element("span", `tip-state ${chipState(session)}`, state.join(" · ")));
  const unread = session.unread > 0 ? ` · ${session.unread} unread` : "";
  tip.replaceChildren(
    head,
    element("div", "", `${session.account} · ${session.directory}`),
    element("div", "", `${session.short_id ?? "another terminal"}${unread}`),
  );
  // Shown before it is placed: a hidden element has no width.
  tip.hidden = false;
  const box = item.getBoundingClientRect();
  tip.style.left = `${Math.max(Math.min(box.left, window.innerWidth - tip.offsetWidth - 8), 8)}px`;
  tip.style.top = `${box.bottom + 6}px`;
}

function hideTip() {
  document.getElementById("tip").hidden = true;
}

// Chips that do not fit are hidden from the end, where the sessions that need you least
// are; "+N" opens Ctrl+P, which has them all.
function fitChips() {
  const chips = document.getElementById("chips");
  const more = chips.querySelector(".more");
  if (!more) return;
  const items = [...chips.children].filter((item) => item !== more);
  for (const item of items) item.hidden = false;
  more.hidden = true;
  let hidden = 0;
  while (chips.scrollWidth > chips.clientWidth && hidden < items.length) {
    hidden += 1;
    items[items.length - hidden].hidden = true;
    more.hidden = false;
    more.textContent = `+${hidden}`;
  }
}

function renderChips(view) {
  const chips = document.getElementById("chips");
  hideTip();
  if (!view.loaded) {
    chips.replaceChildren(element("li", "chip-note", "loading…"));
  } else if (view.sessions.length === 0) {
    chips.replaceChildren(element("li", "chip-note", "no sessions · ctrl+n starts one"));
  } else {
    const more = element("li", "chip more");
    more.addEventListener("click", openPicker);
    chips.replaceChildren(...view.sessions.map(chip), more);
  }
  fitChips();
}

function render(view) {
  lastView = view;
  renderChips(view);
  renderStatus();
  panes.renderBar();
  panes.decorate();
}

// Claude finished a turn in this session and it has not been looked at since.
function isFinished(key) {
  return lastView.sessions.some((row) => row.finished && row.account === key.account && row.short_id === key.short_id);
}

// The title of a session shown in a pane, for the tab bar.
function titleOf(key) {
  const session = lastView.sessions.find((row) => row.account === key.account && row.short_id === key.short_id);
  return session ? session.title : key.short_id;
}

function openSession(session) {
  if (!session.short_id) {
    setStatus(elsewhere(session));
    return Promise.resolve();
  }
  setStatus("");
  return panes.open(session);
}

function showNumber(number) {
  const session = lastView.sessions[number - 1];
  if (!session) {
    setStatus(`no session #${number}`);
    return;
  }
  openSession(session);
}

function openNewSessionForm() {
  const form = document.getElementById("new-form");
  const fields = form.elements;
  fields.account.replaceChildren(...lastView.accounts.map((account) => element("option", "", account)));
  fields.directory.value = "~";
  fields.name.value = "";
  document.getElementById("new-message").textContent = "";
  form.querySelector("button").disabled = false;
  document.getElementById("new-session").showModal();
  fields.account.focus();
}

async function startNewSession(event) {
  event.preventDefault();
  const form = event.target;
  const fields = form.elements;
  const dialog = document.getElementById("new-session");
  const message = document.getElementById("new-message");
  const account = fields.account.value;
  if (!account) {
    message.textContent = "no accounts: claude-accounts add <name>";
    return;
  }
  const button = form.querySelector("button");
  button.disabled = true;
  message.textContent = `starting a session on ${account}…`;
  let created;
  try {
    created = await invoke("new_session", { account, directory: fields.directory.value, name: fields.name.value });
  } catch (error) {
    // Escape may have closed the form while the session was starting.
    if (dialog.open) message.textContent = `${error}`;
    else setStatus(`${error}`);
    button.disabled = false;
    return;
  }
  dialog.close();
  await openSession(created);
  if (created.warnings.length > 0) setStatus(created.warnings.join(" · "));
}

function renderPicker() {
  const results = document.getElementById("picker-results");
  if (picker.rows.length === 0) {
    results.replaceChildren(element("li", "muted", "no match"));
    return;
  }
  results.replaceChildren(
    ...picker.rows.map((session, index) => {
      const item = sessionItem(session);
      if (session.short_id) item.classList.add("openable");
      if (index === picker.selected) item.classList.add("selected");
      item.addEventListener("click", () => choose(session));
      return item;
    }),
  );
  results.children[picker.selected].scrollIntoView({ block: "nearest" });
}

async function filterPicker() {
  // Answers can come back out of order: only the last query counts.
  const query = ++picker.query;
  const rows = await invoke("filter_sessions", { query: document.getElementById("picker-query").value });
  if (query !== picker.query) return;
  picker.rows = rows;
  picker.selected = 0;
  renderPicker();
}

function openPicker() {
  document.getElementById("picker-query").value = "";
  document.getElementById("picker").showModal();
  filterPicker();
}

function choose(session) {
  document.getElementById("picker").close();
  openSession(session);
}

function onPickerKey(event) {
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    const step = event.key === "ArrowDown" ? 1 : -1;
    picker.selected = Math.min(Math.max(picker.selected + step, 0), Math.max(picker.rows.length - 1, 0));
    renderPicker();
  } else if (event.key === "Enter") {
    event.preventDefault();
    const session = picker.rows[picker.selected];
    if (session) choose(session);
  }
}

// Keys the app keeps for itself, like `command_for` in the terminal interface. `code` does
// not depend on the keyboard layout.
function shortcutFor(event) {
  const ctrl = event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
  const alt = event.altKey && !event.ctrlKey && !event.shiftKey && !event.metaKey;
  if (ctrl && event.code === "KeyN") return { run: openNewSessionForm };
  if (ctrl && event.code === "KeyP") return { run: openPicker };
  const digit = /^Digit([1-9])$/.exec(event.code);
  if (alt && digit) return { run: () => showNumber(Number(digit[1])) };
  if (ctrl && event.code === "KeyH") return { run: () => panes.step(-1) };
  if (ctrl && event.code === "KeyL") return { run: () => panes.step(1) };
  const ctrlShift = event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
  if (ctrlShift && event.code === "KeyD") return { run: () => panes.split() };
  if (ctrlShift && event.code === "KeyW") return { run: () => panes.close() };
  if (ctrlShift && event.code === "KeyT") return { run: () => panes.newTab() };
  if (ctrlShift && event.code === "KeyX") return { run: () => panes.closeTab() };
  if (ctrlShift && event.code === "KeyH") return { run: () => panes.tabStep(-1) };
  if (ctrlShift && event.code === "KeyL") return { run: () => panes.tabStep(1) };
  if (ctrlShift && digit) return { run: () => panes.goTab(Number(digit[1])) };
  return null;
}

function onKey(event) {
  const shortcut = shortcutFor(event);
  if (!shortcut) return;
  event.preventDefault();
  if (document.querySelector("dialog[open]")) return;
  shortcut.run();
}

const panes = new Panes(document.getElementById("panes"), document.getElementById("tabs"), {
  keyFilter: (event) => shortcutFor(event) === null,
  onChange: () => render(lastView),
  titleOf,
  isFinished,
});

document.addEventListener("keydown", onKey);
new ResizeObserver(fitChips).observe(document.getElementById("chips"));
document.getElementById("new-form").addEventListener("submit", startNewSession);
document.getElementById("picker-query").addEventListener("input", filterPicker);
document.getElementById("picker-query").addEventListener("keydown", onPickerKey);
for (const dialog of document.querySelectorAll("dialog")) {
  dialog.addEventListener("close", () => panes.focus());
}

// xterm measures its cells with the font it is given: it must be loaded first.
await Promise.all([document.fonts.load('400 13px "JetBrains Mono"'), document.fonts.load('700 13px "JetBrains Mono"')]);
await listen("pane-ended", (event) => panes.ended(event.payload));
await listen("tabs", (event) => panes.render(event.payload));
await listen("sessions", (event) => render(event.payload));
await panes.load();
render(await invoke("current_list"));
