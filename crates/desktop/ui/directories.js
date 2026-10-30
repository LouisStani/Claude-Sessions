// The help under a directory field: the folders already in use until something is typed,
// then the folders that can follow what is typed. Rust reads the disk.

const { invoke } = window.__TAURI__.core;

export class DirectoryField {
  constructor(input, list) {
    this.input = input;
    this.list = list;
    this.items = [];
    this.selected = -1;
    // Answers can come back out of order: only the last request counts.
    this.request = 0;
    this.edited = false;
    input.addEventListener("focus", () => this.refresh());
    input.addEventListener("input", () => {
      this.edited = true;
      this.complete();
    });
    input.addEventListener("keydown", (event) => this.onKey(event));
    input.addEventListener("blur", () => this.show([]));
  }

  // A dialog opens with a proposed folder: the known folders come first again. They show
  // once the field has the focus, without waiting for a focus event the window may not send.
  reset() {
    this.edited = false;
    this.show([]);
    this.refresh();
  }

  refresh() {
    if (this.edited) this.complete();
    else this.ask("known_directories", {});
  }

  complete() {
    this.ask("complete_directory", { text: this.input.value });
  }

  async ask(command, args) {
    const request = ++this.request;
    const items = await invoke(command, args);
    if (request === this.request && document.activeElement === this.input) this.show(items);
  }

  show(items) {
    this.items = items;
    this.selected = -1;
    this.render();
  }

  render() {
    this.list.replaceChildren(
      ...this.items.map((item, index) => {
        const entry = document.createElement("li");
        entry.className = index === this.selected ? "suggestion selected" : "suggestion";
        entry.textContent = item;
        // Keeps the focus in the field, which would hide the list.
        entry.addEventListener("mousedown", (event) => event.preventDefault());
        entry.addEventListener("click", () => this.choose(item));
        return entry;
      }),
    );
    this.list.hidden = this.items.length === 0;
    if (this.selected >= 0) this.list.children[this.selected].scrollIntoView({ block: "nearest" });
  }

  // The folder goes in the field and its own folders are listed next.
  choose(item) {
    this.input.value = item;
    this.edited = true;
    this.complete();
  }

  onKey(event) {
    if (this.items.length === 0) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const count = this.items.length;
      if (this.selected < 0) this.selected = event.key === "ArrowDown" ? 0 : count - 1;
      else this.selected = (this.selected + (event.key === "ArrowDown" ? 1 : -1) + count) % count;
      this.render();
    } else if (event.key === "Tab" && !event.shiftKey) {
      event.preventDefault();
      this.choose(this.items[Math.max(this.selected, 0)]);
    } else if (event.key === "Enter" && this.selected >= 0) {
      event.preventDefault();
      this.choose(this.items[this.selected]);
    }
  }
}
