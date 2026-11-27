const diagrams = document.querySelectorAll("pre > code.language-mermaid");

if (diagrams.length > 0) {
  const { default: mermaid } = await import(
    "https://cdn.jsdelivr.net/npm/mermaid@12.3.0/dist/mermaid.esm.min.mjs"
  );
  mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });

  for (const [index, diagram] of diagrams.entries()) {
    const { svg } = await mermaid.render(`flow-diagram-${index}`, diagram.textContent);
    const container = document.createElement("div");
    container.innerHTML = svg;
    diagram.parentElement.replaceWith(container);
  }
}
