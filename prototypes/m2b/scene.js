/* Presentation-only fixture. No source mutation, command dispatch, undo or network. */
"use strict";
const scene = JSON.parse(document.getElementById("scene-data").textContent);
const workspace = document.getElementById("workspace");
const palette = document.getElementById("palette");
const shade = document.getElementById("palette-shade");
const workspaceButton = document.getElementById("workspace-state");
const paletteButton = document.getElementById("palette-state");
const referenceButton = document.getElementById("reference-state");
const compactButton = document.getElementById("compact-state");

function sourceRows(target, source, view) {
  const container = document.getElementById(target);
  source.lines.forEach((line, index) => {
    const row = document.createElement("div");
    row.className = "source-line";
    if (index === view.cursor.line) row.classList.add("current-line");
    // Explicit fixture styles only; this is not syntax analysis of user input.
    if (index === 0) row.classList.add("comment");
    if (line.startsWith("{") || line.startsWith("&")) row.classList.add("directive");
    const number = document.createElement("span");
    number.className = "line-number";
    number.textContent = String(index + 1);
    number.setAttribute("aria-hidden", "true");
    const content = document.createElement("span");
    content.className = "line-content";
    const highlight = source.highlights.find(hit => hit.line === index);
    if (highlight) {
      const scalars = Array.from(line);
      content.append(document.createTextNode(scalars.slice(0, highlight.scalarStart).join("")));
      const hit = document.createElement("mark");
      hit.className = "search-hit";
      hit.textContent = scalars.slice(highlight.scalarStart, highlight.scalarStart + highlight.scalarLength).join("");
      content.append(hit, document.createTextNode(scalars.slice(highlight.scalarStart + highlight.scalarLength).join("")));
    } else {
      content.textContent = line || " ";
    }
    row.append(number, content);
    container.append(row);
  });
}
sourceRows("primary-source", scene.documents[0], scene.views[0]);
sourceRows("reference-source", scene.documents[1], scene.views[1]);
scene.documents[0].highlights.forEach(hit => {
  const row = document.createElement("div");
  row.className = "result-row";
  const position = document.createElement("span");
  position.className = "result-location";
  position.textContent = `order-entry.p:${hit.line + 1}:${hit.scalarStart + 1}`;
  const snippet = document.createElement("span");
  snippet.className = "result-snippet";
  snippet.textContent = scene.documents[0].lines[hit.line].trim();
  row.append(position, snippet);
  document.getElementById("search-results").append(row);
});
function showPalette(visible) {
  palette.hidden = !visible;
  shade.hidden = !visible;
  paletteButton.setAttribute("aria-pressed", String(visible));
  workspaceButton.setAttribute("aria-pressed", String(!visible));
}
workspaceButton.addEventListener("click", () => showPalette(false));
paletteButton.addEventListener("click", () => showPalette(true));
referenceButton.addEventListener("click", () => {
  const reference = workspace.classList.toggle("reference-focused");
  referenceButton.setAttribute("aria-pressed", String(reference));
  document.getElementById("primary-group").classList.toggle("active", !reference);
  document.getElementById("reference-group").classList.toggle("active", reference);
  const view = scene.views[reference ? 1 : 0];
  document.getElementById("status-focus").textContent = reference ? "Grupo 2 · referencia" : "Grupo 1";
  document.getElementById("status-position").textContent = `Ln ${view.cursor.line + 1}, Col ${view.cursor.scalarColumn + 1}`;
});
compactButton.addEventListener("click", () => {
  compactButton.setAttribute("aria-pressed", String(workspace.classList.toggle("compact")));
});
document.addEventListener("keydown", event => {
  // Only the study overlay closes; no editor shortcuts or commands are emulated.
  if (event.key === "Escape") showPalette(false);
});
