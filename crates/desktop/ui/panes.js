// The tabs and panes on the right. Rust owns them (tabs.rs, layout.rs): every change is a
// command, and this module only draws what it gets back.

import { mark } from "./trace.js";

const { invoke, Channel } = window.__TAURI__.core;

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function sameSession(key, session) {
  return key !== null && key.account === session.account && key.short_id === session.short_id;
}

// The terminal colors come from the art direction in style.css, so a theme changes them too.
function terminalOptions() {
  const style = getComputedStyle(document.documentElement);
  const variable = (name) => style.getPropertyValue(name).trim();
  return {
    fontFamily: variable("--font"),
    fontSize: 13,
    theme: {
      background: variable("--terminal"),
      foreground: variable("--terminal-text"),
      cursor: variable("--waiting"),
      cursorAccent: variable("--terminal"),
      selectionBackground: variable("--selection"),
    },
  };
}

function loadWebgl(terminal) {
  try {
    const webgl = new WebglAddon.WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    terminal.loadAddon(webgl);
  } catch (error) {
    console.warn("WebGL unavailable, using the DOM renderer:", error);
  }
}

// One pane: a terminal kept across renders, and the session attached in it. `current.output`
// tells its bytes apart from those of the session shown before, `current.generation` its end.
class PaneView {
  constructor(id, keyFilter) {
    this.id = id;
    this.current = null;
    // A session can end before `open_session` has returned its generation.
    this.lastEnded = null;
    this.used = false;
    this.element = element("div", "pane");
    this.title = element("span", "head-title");
    this.account = element("span", "head-account");
    this.done = element("span", "head-done", "done");
    const head = element("div", "pane-head");
    head.append(element("span", "prompt", "~ $"), this.title, this.account, this.done);
    this.screen = element("div", "screen");
    this.placeholder = element("p", "placeholder", "select a session");
    this.element.append(head, this.screen, this.placeholder);
    this.terminal = new Terminal(terminalOptions());
    this.fit = new FitAddon.FitAddon();
    this.terminal.loadAddon(this.fit);
    this.terminal.attachCustomKeyEventHandler(keyFilter);
    this.terminal.onData((data) => this.forward("write_input", { data }));
    this.terminal.onResize(({ cols, rows }) => this.forward("resize_pane", { columns: cols, rows }));
    this.observer = new ResizeObserver(() => this.fit.fit());
    this.opened = false;
  }

  // xterm measures its cells when opened: only once the pane is in the page.
  openOnce() {
    if (this.opened) return;
    this.opened = true;
    this.terminal.open(this.screen);
    loadWebgl(this.terminal);
    this.observer.observe(this.screen);
    this.fit.fit();
  }

  // The head names the session shown, and says when it finished a turn not looked at yet.
  label(key, title, finished) {
    this.title.textContent = key === null ? "empty" : title;
    this.account.textContent = key === null ? "" : `· ${key.account}`;
    this.done.hidden = !finished;
    this.element.classList.toggle("finished", finished);
  }

  forward(command, args) {
    if (!this.current || this.current.ended) return;
    invoke(command, { pane: this.id, ...args }).catch((error) => console.error(error));
  }

  showEnd() {
    this.current.ended = true;
    this.terminal.write("\r\n\x1b[2m[session closed]\x1b[0m\r\n");
  }

  dispose() {
    this.observer.disconnect();
    this.terminal.dispose();
  }
}

function leaves(node, found = []) {
  if (node.kind === "leaf") found.push(node);
  else {
    leaves(node.first, found);
    leaves(node.second, found);
  }
  return found;
}

export class Panes {
  constructor(container, bar, { keyFilter, onChange, titleOf, isFinished, markOf }) {
    this.container = container;
    this.bar = bar;
    this.keyFilter = keyFilter;
    this.onChange = onChange;
    this.titleOf = titleOf;
    this.isFinished = isFinished;
    this.markOf = markOf;
    this.state = null;
    this.views = new Map();
  }

  async load() {
    this.render(await invoke("tabs"));
  }

  // The visible tab: its `project` is null for "other".
  activeTab() {
    return this.state.tabs.find((tab) => tab.id === this.state.active);
  }

  layout() {
    return this.activeTab().layout;
  }

  // The session shown in the active pane of the visible tab, or null.
  activeSession() {
    const layout = this.layout();
    const leaf = leaves(layout.root).find((node) => node.id === layout.active);
    return leaf ? leaf.session : null;
  }

  projectName(id) {
    const tab = this.state === null ? null : this.state.tabs.find((tab) => tab.id === id);
    return tab && tab.project ? tab.project.name : null;
  }

  allLeaves() {
    return this.state.tabs.flatMap((tab) => leaves(tab.layout.root));
  }

  view(id) {
    if (!this.views.has(id)) this.views.set(id, new PaneView(id, this.keyFilter));
    return this.views.get(id);
  }

  active() {
    return this.view(this.layout().active);
  }

  shows(session) {
    return this.state !== null && this.allLeaves().some((leaf) => sameSession(leaf.session, session));
  }

  draw(node, layout) {
    if (node.kind === "leaf") {
      const view = this.view(node.id);
      view.element.classList.toggle("active", node.id === layout.active);
      view.placeholder.hidden = node.session !== null || view.used;
      view.element.onmousedown = () => this.focusPane(node.id);
      return view.element;
    }
    const split = element("div", `split ${node.direction}`);
    const first = this.draw(node.first, layout);
    const second = this.draw(node.second, layout);
    first.style.flex = `${node.ratio} 1 0px`;
    second.style.flex = `${1 - node.ratio} 1 0px`;
    const divider = element("div", "divider");
    divider.onmousedown = (event) => this.drag(event, node, split, first, second);
    split.append(first, divider, second);
    return split;
  }

  // Every tab stays in the page, hidden ones with `display: none`: their terminals keep
  // receiving output and are never moved.
  render(state) {
    const start = performance.now();
    this.state = state;
    const ids = new Set(this.allLeaves().map((leaf) => leaf.id));
    for (const [id, view] of this.views) {
      if (!ids.has(id)) {
        view.dispose();
        this.views.delete(id);
      }
    }
    const pages = state.tabs.map((tab) => {
      const page = element("div", "tab-page");
      page.hidden = tab.id !== state.active;
      const root = this.draw(tab.layout.root, tab.layout);
      // A pane lifted to the root after a close still has the share it had in its split.
      root.style.flex = "";
      page.append(root);
      return page;
    });
    this.container.replaceChildren(...pages);
    // xterm measures its cells when opened, so only panes on screen are opened; every pane
    // is created in the active tab.
    for (const leaf of leaves(this.layout().root)) this.view(leaf.id).openOnce();
    this.renderBar();
    this.decorate();
    this.focus();
    this.onChange();
    mark("render panes", start);
  }

  // Names the session of every pane, and marks those that finished a turn not looked at.
  decorate() {
    if (this.state === null) return;
    for (const leaf of this.allLeaves()) {
      const key = leaf.session;
      const title = key === null ? "" : this.titleOf(key);
      this.view(leaf.id).label(key, title, key !== null && this.isFinished(key));
    }
  }

  // "n name" per tab, which carries the dot of its most urgent session.
  renderBar() {
    if (this.state === null) return;
    this.bar.replaceChildren(
      ...this.state.tabs.map((tab, index) => {
        const button = element("button", "tab");
        button.append(element("span", "pos", `${index + 1}`), element("span", "tab-name", tab.project ? tab.project.name : "other"));
        button.title = tab.project ? tab.project.directory : "sessions of no project";
        const mark = this.markOf(tab.id);
        if (mark) {
          button.classList.add(mark);
          button.append(element("span", "dot", mark === "finished" ? "✓" : ""));
        }
        button.classList.toggle("active", tab.id === this.state.active);
        button.onclick = () => this.goTab(index + 1);
        return button;
      }),
    );
  }

  focus() {
    if (!document.querySelector("dialog[open]")) this.active().terminal.focus();
  }

  // The separator follows the mouse; Rust gets the ratio once it is released.
  drag(event, node, split, first, second) {
    event.preventDefault();
    const row = node.direction === "row";
    let ratio = node.ratio;
    const move = (moved) => {
      const box = split.getBoundingClientRect();
      const offset = row ? moved.clientX - box.left : moved.clientY - box.top;
      ratio = Math.min(Math.max(offset / (row ? box.width : box.height), 0.1), 0.9);
      first.style.flex = `${ratio} 1 0px`;
      second.style.flex = `${1 - ratio} 1 0px`;
    };
    const release = async () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", release);
      this.render(await invoke("resize_split", { split: node.id, ratio }));
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", release);
  }

  async focusPane(id) {
    if (id !== this.layout().active) this.render(await invoke("focus_pane", { pane: id }));
  }

  async step(step) {
    this.render(await invoke("focus_step", { step }));
  }

  async split() {
    const box = this.active().element.getBoundingClientRect();
    this.render(await invoke("split_pane", { width: box.width, height: box.height }));
  }

  async close() {
    this.render(await invoke("close_pane"));
  }

  async addProject(directory) {
    this.render(await invoke("add_project", { directory }));
  }

  async closeTab() {
    this.render(await invoke("close_tab"));
  }

  async tabStep(step) {
    this.render(await invoke("tab_step", { step }));
  }

  async goTab(number) {
    this.render(await invoke("go_tab", { number }));
  }

  async goTabId(id) {
    const index = this.state.tabs.findIndex((tab) => tab.id === id);
    if (index >= 0 && id !== this.state.active) await this.goTab(index + 1);
  }

  // Shows `session` in the active pane. If a pane of any tab shows it already, Rust brings
  // that tab and pane to the front instead.
  async open(session) {
    const start = performance.now();
    const view = this.active();
    const request = {
      pane: view.id,
      account: session.account,
      shortId: session.short_id,
      columns: view.terminal.cols,
      rows: view.terminal.rows,
    };
    if (this.shows(session)) {
      const opened = await invoke("open_session", { ...request, output: new Channel() });
      this.render(opened.tabs);
      return;
    }
    const output = new Channel();
    const opening = { output, generation: null, ended: false };
    view.current = opening;
    view.used = true;
    let shown = false;
    output.onmessage = (bytes) => {
      if (view.current !== opening) return;
      view.terminal.write(new Uint8Array(bytes), shown ? undefined : () => mark("first output shown", start));
      shown = true;
    };
    view.placeholder.hidden = true;
    view.terminal.reset();
    let opened;
    try {
      opened = await invoke("open_session", { ...request, output });
    } catch (error) {
      if (view.current !== opening) return;
      view.showEnd();
      view.terminal.write(`\x1b[31m${error}\x1b[0m\r\n`);
      return;
    }
    // Null: another pane showed it already (this page had not seen it yet) and has the focus.
    if (opened.generation === null && view.current === opening) view.current = null;
    opening.generation = opened.generation;
    if (view.current === opening && view.lastEnded === opening.generation) view.showEnd();
    this.render(opened.tabs);
  }

  ended({ pane, generation }) {
    const view = this.views.get(pane);
    if (!view) return;
    view.lastEnded = generation;
    if (view.current && view.current.generation === generation) view.showEnd();
  }
}
