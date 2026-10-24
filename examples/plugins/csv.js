// A "csv" card kind: `tersh-show -k csv data.csv` renders a table.
// Install: copy to ~/.config/tersh/plugins/csv.js

function parse(text) {
  const rows = [];
  let row = [];
  let field = "";
  let quoted = false;
  const endField = () => {
    row.push(field);
    field = "";
  };
  const endRow = () => {
    endField();
    rows.push(row);
    row = [];
  };
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (ch === '"') {
        quoted = false;
      } else {
        field += ch;
      }
    } else if (ch === '"') {
      quoted = true;
    } else if (ch === ",") {
      endField();
    } else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && text[i + 1] === "\n") i++;
      endRow();
    } else {
      field += ch;
    }
  }
  if (field || row.length) endRow();
  return rows;
}

export function activate(tersh) {
  tersh.addStyle(`
    .csv { border-collapse: collapse; margin: 6px 10px; font-family: var(--font-family); font-size: 12px; }
    .csv th { position: sticky; top: 0; background: var(--card-bg); color: var(--accent); text-align: left; }
    .csv th, .csv td { padding: 2px 10px; border-bottom: 1px solid var(--card-border); white-space: nowrap; }
    .csv td.num { text-align: right; color: var(--ansi-14); }
  `);

  tersh.registerRenderer("csv", ({ el, data, relayout }) => {
    const render = (value) => {
      const [header = [], ...body] = parse(String(value ?? ""));
      const table = document.createElement("table");
      table.className = "csv";
      const head = table.createTHead().insertRow();
      for (const name of header) head.append(Object.assign(document.createElement("th"), { textContent: name }));
      const tbody = table.createTBody();
      for (const cells of body) {
        const tr = tbody.insertRow();
        for (const cell of cells) {
          const td = tr.insertCell();
          td.textContent = cell;
          if (cell.trim() !== "" && !isNaN(Number(cell))) td.className = "num";
        }
      }
      el.replaceChildren(table);
      relayout();
    };
    render(data);
    return { update: render };
  });
}
