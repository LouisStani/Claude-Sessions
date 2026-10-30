// The panes on the right. Rust owns the tree (layout.rs): every change is a command, and this
// module only draws the tree it gets back.

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
    this.screen = element("div", "screen");
    this.placeholder = element("p", "placeholder", "select a session");
    this.element.append(this.screen, this.placeholder);
    this.terminal = new Terminal({ fontFamily: "monospace", fontSize: 14, theme: { background: "#111111" } });
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

export class Panes {
  constructor(container, { keyFilter, onChange }) {
    this.container = container;
    this.keyFilter = keyFilter;
    this.onChange = onChange;
    this.layout = null;
    this.views = new Map();
  }

  async load() {
    this.render(await invoke("layout"));
  }

  leaves(node = this.layout.root, found = []) {
    if (node.kind === "leaf") found.push(node);
    else {
      this.leaves(node.first, found);
      this.leaves(node.second, found);
    }
    return found;
  }

  view(id) {
    if (!this.views.has(id)) this.views.set(id, new PaneView(id, this.keyFilter));
    return this.views.get(id);
  }

  active() {
    return this.view(this.layout.active);
  }

  shows(session) {
    return this.layout !== null && this.leaves().some((leaf) => sameSession(leaf.session, session));
  }

  draw(node) {
    if (node.kind === "leaf") {
      const view = this.view(node.id);
      view.element.classList.toggle("active", node.id === this.layout.active);
      view.placeholder.hidden = node.session !== null || view.used;
      view.element.onmousedown = () => this.focusPane(node.id);
      return view.element;
    }
    const split = element("div", `split ${node.direction}`);
    const first = this.draw(node.first);
    const second = this.draw(node.second);
    first.style.flex = `${node.ratio} 1 0px`;
    second.style.flex = `${1 - node.ratio} 1 0px`;
    const divider = element("div", "divider");
    divider.onmousedown = (event) => this.drag(event, node, split, first, second);
    split.append(first, divider, second);
    return split;
  }

  render(layout) {
    this.layout = layout;
    const ids = new Set(this.leaves().map((leaf) => leaf.id));
    for (const [id, view] of this.views) {
      if (!ids.has(id)) {
        view.dispose();
        this.views.delete(id);
      }
    }
    const root = this.draw(layout.root);
    // A pane lifted to the root after a close still has the share it had in its split.
    root.style.flex = "";
    this.container.replaceChildren(root);
    for (const id of ids) this.view(id).openOnce();
    this.focus();
    this.onChange();
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
    if (id !== this.layout.active) this.render(await invoke("focus_pane", { pane: id }));
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

  // Shows `session` in the active pane, or moves the focus to the pane already showing it.
  async open(session) {
    const shownIn = this.leaves().find((leaf) => sameSession(leaf.session, session));
    if (shownIn) {
      await this.focusPane(shownIn.id);
      return;
    }
    const view = this.active();
    const output = new Channel();
    const opening = { output, generation: null, ended: false };
    view.current = opening;
    view.used = true;
    output.onmessage = (bytes) => {
      if (view.current === opening) view.terminal.write(new Uint8Array(bytes));
    };
    view.placeholder.hidden = true;
    view.terminal.reset();
    let opened;
    try {
      opened = await invoke("open_session", {
        pane: view.id,
        account: session.account,
        shortId: session.short_id,
        columns: view.terminal.cols,
        rows: view.terminal.rows,
        output,
      });
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
    this.render(opened.layout);
  }

  ended({ pane, generation }) {
    const view = this.views.get(pane);
    if (!view) return;
    view.lastEnded = generation;
    if (view.current && view.current.generation === generation) view.showEnd();
  }
}
