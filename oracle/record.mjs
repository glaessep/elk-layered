// Lays out every `<case>.input.json` in a directory with elkjs 0.9.3 and writes what elkjs
// returns to `<case>.expected.json` beside it. `tests/elkjs_parity.rs` then holds elk-layered to
// those files, number for number.
//
//   cd oracle && npm install && node record.mjs ../tests/elkjs
//
// Record again only when a case's input changes. Never edit an expected file by hand. elkjs is
// installed here to record and nowhere else: it is never committed and never part of the crate.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import ELK from "elkjs/lib/elk.bundled.js";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node record.mjs <directory of *.input.json>");
  process.exit(2);
}

const elk = new ELK();
const inputs = readdirSync(dir)
  .filter((name) => name.endsWith(".input.json"))
  .sort();
if (inputs.length === 0) {
  console.error(`no *.input.json in ${dir}`);
  process.exit(1);
}

for (const name of inputs) {
  const graph = JSON.parse(readFileSync(join(dir, name), "utf8"));
  const laid = await elk.layout(graph);
  const out = join(dir, name.replace(/\.input\.json$/, ".expected.json"));
  writeFileSync(out, JSON.stringify(laid, null, 2) + "\n");
  console.log(`recorded ${out}`);
}
