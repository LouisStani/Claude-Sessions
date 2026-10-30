const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

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
  return item;
}

function render(view) {
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

await listen("sessions", (event) => render(event.payload));
render(await invoke("current_list"));
