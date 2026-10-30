import { DirectoryField } from "./directories.js";
import { Panes, sameSession } from "./panes.js";
import { mark, startTrace, traced } from "./trace.js";

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
  return `"${session.label}" runs in another terminal: it cannot be opened here`;
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

function directoryField(formId) {
  const form = document.getElementById(formId);
  return new DirectoryField(form.elements.directory, form.querySelector(".suggestions"));
}

// The sessions of the bar: those of the visible tab, its project's or, in "other", those of
// no project. Rust sorts them, most urgent first.
function barSessions() {
  if (panes.state === null) return [];
  const tab = panes.activeTab();
  return lastView.sessions.filter((session) => !session.hidden && session.tab === tab.id);
}

// The dot of a tab: its most urgent session, if that one needs you.
function markOf(tabId) {
  const session = lastView.sessions.find((row) => row.tab === tabId && !row.hidden);
  if (!session) return null;
  if (session.unread > 0) return "waiting";
  const state = chipState(session);
  return state === "finished" || state === "waiting" ? state : null;
}

function sessionItem(session) {
  const item = element("li", "session");
  const title = element("div", "title");
  title.append(element("span", "name", session.label));
  if (session.unread > 0) title.append(element("span", "badge", `${session.unread} msg`));
  if (session.finished) title.append(element("span", "done", "done"));
  const detail = [session.account, session.state];
  if (session.isolated) detail.push("isolated");
  const project = panes.projectName(session.tab);
  if (project) detail.push(project);
  if (session.hidden) detail.push("hidden");
  item.append(title, element("div", "detail", detail.join(" · ")));
  return item;
}

function chip(session, index) {
  const item = element("li", `chip ${chipState(session)}`);
  item.append(element("span", "pos", index < 9 ? `${index + 1}` : ""));
  item.append(element("span", "dot", session.finished ? "✓" : ""));
  item.append(element("span", "chip-title", session.label));
  if (session.unread > 0) item.append(element("span", "badge", `${session.unread}`));
  if (panes.shows(session)) item.classList.add("open");
  item.addEventListener("click", () => traced("chip click", () => openSession(session)));
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
// are; "+N" opens Ctrl+P, which has them all. Chips never shrink, and hiding the last ones
// does not move the first: two measures are enough.
function fitChips() {
  const chips = document.getElementById("chips");
  const more = chips.querySelector(".more");
  if (!more) return;
  const items = [...chips.children].filter((item) => item !== more);
  for (const item of items) item.hidden = false;
  more.hidden = true;
  // Not laid out yet: nothing to measure, every chip stays.
  if (chips.clientWidth === 0) return;
  // Right edges rather than scrollWidth, which engines round differently.
  const edge = chips.getBoundingClientRect().right + 0.5;
  const rights = items.map((item) => item.getBoundingClientRect().right);
  if (rights.length === 0 || rights[rights.length - 1] <= edge) return;
  // "+N" as wide as it can get, after the gap between chips.
  more.textContent = `+${items.length}`;
  more.hidden = false;
  const room = edge - more.getBoundingClientRect().width - parseFloat(getComputedStyle(chips).columnGap || "0");
  const kept = rights.filter((right) => right <= room).length;
  for (const item of items.slice(kept)) item.hidden = true;
  more.textContent = `+${items.length - kept}`;
}

function renderChips(view) {
  const chips = document.getElementById("chips");
  hideTip();
  const sessions = barSessions();
  if (!view.loaded) {
    chips.replaceChildren(element("li", "chip-note", "loading…"));
  } else if (sessions.length === 0) {
    chips.replaceChildren(element("li", "chip-note", "no sessions here · ctrl+n starts one"));
  } else {
    const more = element("li", "chip more");
    more.addEventListener("click", openPicker);
    chips.replaceChildren(...sessions.map(chip), more);
  }
  fitChips();
}

function render(view) {
  const start = performance.now();
  lastView = view;
  renderChips(view);
  renderStatus();
  panes.renderBar();
  panes.decorate();
  mark("render sessions", start);
}

// Claude finished a turn in this session and it has not been looked at since.
function isFinished(key) {
  return lastView.sessions.some((row) => row.finished && row.account === key.account && row.short_id === key.short_id);
}

function rowOf(key) {
  return lastView.sessions.find((row) => row.account === key.account && row.short_id === key.short_id);
}

// The name of a session shown in a pane, for its head.
function titleOf(key) {
  const session = rowOf(key);
  return session ? session.label : key.short_id;
}

// A session opens in the tab of its project, "other" for no project. A
// session just started is not in the list yet: it opens where it was started.
async function openSession(session) {
  if (!session.short_id) {
    setStatus(elsewhere(session));
    return;
  }
  setStatus("");
  if (session.tab !== undefined) await panes.goTabId(session.tab);
  await panes.open(session);
}

function showNumber(number) {
  const session = barSessions()[number - 1];
  if (!session) {
    setStatus(`no session #${number}`);
    return;
  }
  return openSession(session);
}

// The session `step` places away from `key` among those that can open here, round the
// end; from an empty pane, the first or the last. Null when there is nothing else to open.
export function neighbour(sessions, key, step) {
  const openable = sessions.filter((session) => session.short_id);
  const at = key === null ? -1 : openable.findIndex((session) => sameSession(key, session));
  if (at === -1) return openable.length === 0 ? null : openable[step > 0 ? 0 : openable.length - 1];
  if (openable.length === 1) return null;
  return openable[(at + step + openable.length) % openable.length];
}

// Alt+H / Alt+L: the previous or next chip of the bar, in the active pane.
function stepSession(step) {
  const session = neighbour(barSessions(), panes.activeSession(), step);
  if (!session) {
    setStatus("no other session here");
    return;
  }
  return openSession(session);
}

function openNewSessionForm() {
  const form = document.getElementById("new-form");
  const fields = form.elements;
  fields.account.replaceChildren(...lastView.accounts.map((account) => element("option", "", account)));
  const project = panes.activeTab().project;
  fields.directory.value = project ? project.directory : "~";
  sessionDirectory.reset();
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

function openProjectForm() {
  const form = document.getElementById("project-form");
  form.elements.directory.value = "~";
  projectDirectory.reset();
  document.getElementById("project-message").textContent = "";
  document.getElementById("new-project").showModal();
  form.elements.directory.focus();
}

async function addProject(event) {
  event.preventDefault();
  const dialog = document.getElementById("new-project");
  try {
    await panes.addProject(event.target.elements.directory.value);
  } catch (error) {
    document.getElementById("project-message").textContent = `${error}`;
    return;
  }
  dialog.close();
}

async function closeTab() {
  try {
    await panes.closeTab();
  } catch (error) {
    setStatus(`${error}`);
  }
}

function openRenameForm() {
  const key = panes.activeSession();
  const session = key && rowOf(key);
  if (!session) {
    setStatus("no session in this pane to name");
    return;
  }
  const form = document.getElementById("rename-form");
  form.dataset.account = session.account;
  form.dataset.shortId = session.short_id;
  form.elements.label.value = session.given ?? "";
  document.getElementById("rename-message").textContent = "";
  document.getElementById("rename").showModal();
  form.elements.label.focus();
}

async function renameSession(event) {
  event.preventDefault();
  const form = event.target;
  try {
    await invoke("rename_session", { account: form.dataset.account, shortId: form.dataset.shortId, name: form.elements.label.value });
  } catch (error) {
    document.getElementById("rename-message").textContent = `${error}`;
    return;
  }
  document.getElementById("rename").close();
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
  const rows = await invoke("filter_sessions", { tab: panes.state.active, query: document.getElementById("picker-query").value });
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

// A hidden session chosen here comes back to the bar first.
async function choose(session) {
  document.getElementById("picker").close();
  if (session.hidden) {
    try {
      await invoke("show_session", { account: session.account, shortId: session.short_id });
    } catch (error) {
      setStatus(`${error}`);
      return;
    }
  }
  openSession(session);
}

// Asks before stopping and hiding the session of the active pane.
function confirmHide() {
  const key = panes.activeSession();
  const session = key && rowOf(key);
  if (!session) {
    setStatus("no session in this pane to hide");
    return;
  }
  const form = document.getElementById("hide-form");
  form.dataset.account = session.account;
  form.dataset.shortId = session.short_id;
  document.getElementById("hide-question").textContent =
    `Stop "${session.label}" and hide it from the bar? Its conversation is kept; ctrl+p brings it back.`;
  document.getElementById("confirm-hide").showModal();
  form.querySelector("button[type=submit]").focus();
}

// Stops the session and hides it; Ctrl+P brings it back.
async function hideSession(event) {
  event.preventDefault();
  document.getElementById("confirm-hide").close();
  const form = event.target;
  const session = rowOf({ account: form.dataset.account, short_id: form.dataset.shortId });
  if (!session) return;
  setStatus(`stopping ${session.label}…`);
  try {
    panes.render(await invoke("hide_session", { account: session.account, shortId: session.short_id }));
  } catch (error) {
    setStatus(`${error}`);
    return;
  }
  setStatus(`${session.label} hidden · ctrl+p brings it back`);
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
  const bare = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
  if (bare && event.code === "F2") return { run: openRenameForm };
  const digit = /^Digit([1-9])$/.exec(event.code);
  if (alt && digit) return { run: () => showNumber(Number(digit[1])) };
  if (alt && event.code === "KeyH") return { run: () => stepSession(-1) };
  if (alt && event.code === "KeyL") return { run: () => stepSession(1) };
  if (ctrl && event.code === "KeyH") return { run: () => panes.step(-1) };
  if (ctrl && event.code === "KeyL") return { run: () => panes.step(1) };
  const ctrlShift = event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
  if (ctrlShift && event.code === "KeyD") return { run: () => panes.split() };
  if (ctrlShift && event.code === "KeyW") return { run: () => panes.close() };
  if (ctrlShift && event.code === "KeyT") return { run: openProjectForm };
  if (ctrlShift && event.code === "KeyX") return { run: closeTab };
  if (ctrlShift && event.code === "KeyK") return { run: confirmHide };
  if (ctrlShift && event.code === "KeyH") return { run: () => panes.tabStep(-1) };
  if (ctrlShift && event.code === "KeyL") return { run: () => panes.tabStep(1) };
  if (ctrlShift && digit) return { run: () => panes.goTab(Number(digit[1])) };
  return null;
}

// "ctrl+shift+t", "alt+3", "f2": how a shortcut is named in the trace.
function keyName(event) {
  const modifiers = [event.ctrlKey && "ctrl", event.shiftKey && "shift", event.altKey && "alt"].filter(Boolean);
  const key = event.code.replace(/^(Key|Digit)/, "").toLowerCase();
  return [...modifiers, key].join("+");
}

function onKey(event) {
  const shortcut = shortcutFor(event);
  if (!shortcut) return;
  event.preventDefault();
  if (document.querySelector("dialog[open]")) return;
  traced(`key ${keyName(event)}`, shortcut.run);
}

const panes = new Panes(document.getElementById("panes"), document.getElementById("tabs"), {
  keyFilter: (event) => shortcutFor(event) === null,
  onChange: () => render(lastView),
  titleOf,
  isFinished,
  markOf,
});

const sessionDirectory = directoryField("new-form");
const projectDirectory = directoryField("project-form");

document.addEventListener("keydown", onKey);
new ResizeObserver(fitChips).observe(document.getElementById("chips"));
document.getElementById("new-form").addEventListener("submit", startNewSession);
document.getElementById("project-form").addEventListener("submit", addProject);
document.getElementById("rename-form").addEventListener("submit", renameSession);
document.getElementById("hide-form").addEventListener("submit", hideSession);
document.getElementById("hide-cancel").addEventListener("click", () => document.getElementById("confirm-hide").close());
document.getElementById("picker-query").addEventListener("input", filterPicker);
document.getElementById("picker-query").addEventListener("keydown", onPickerKey);
for (const dialog of document.querySelectorAll("dialog")) {
  dialog.addEventListener("close", () => panes.focus());
}

// xterm measures its cells with the font it is given: it must be loaded first.
await Promise.all([document.fonts.load('400 13px "JetBrains Mono"'), document.fonts.load('700 13px "JetBrains Mono"')]);
await startTrace();
await listen("pane-ended", (event) => panes.ended(event.payload));
await listen("tabs", (event) => panes.render(event.payload));
await listen("sessions", (event) => render(event.payload));
await panes.load();
render(await invoke("current_list"));
// Since the page started loading.
mark("page ready", 0);
