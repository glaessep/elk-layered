// Lays out every `<case>.input.json` in a directory with elkjs 0.9.3 and writes what elkjs
// returns to `<case>.expected.json` beside it. `tests/elkjs_parity.rs` then holds elk-layered to
// those files, number for number.
//
//   cd oracle && npm install && node record.mjs ../tests/elkjs
//
// Record again only when a case's input changes. Never edit an expected file by hand. elkjs is
// installed here to record and nowhere else: it is never committed and never part of the crate.
//
// Each case runs in a fresh Node process. elkjs remembers what it laid out before: GWT numbers
// every object it creates from one global counter, and that number is the object's hash code. So
// the order of ELK's hash sets, and with it a layout, depends on the graphs laid out earlier in
// the same process. A fresh process gives each case the layout a caller sees on its first run.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const [, , dir, one] = process.argv;
if (!dir) {
  console.error("usage: node record.mjs <directory of *.input.json>");
  process.exit(2);
}

if (one) {
  // The child: lay out one case and write it.
  const { default: ELK } = await import("elkjs/lib/elk.bundled.js");
  const graph = JSON.parse(readFileSync(join(dir, one), "utf8"));
  const out = join(dir, one.replace(/\.input\.json$/, ".expected.json"));
  let laid;
  try {
    laid = await new ELK().layout(graph);
  } catch (error) {
    // elkjs refuses the graph. The expected file says so, and the parity test then expects
    // layout() to refuse it too. Only the exception's first line is kept: the rest prints
    // GWT's object dumps, which change with every build.
    laid = { elkjsError: String(error?.message ?? error).split("\n")[0] };
  }
  writeFileSync(out, JSON.stringify(laid, null, 2) + "\n");
  console.log(`recorded ${out}`);
} else {
  const inputs = readdirSync(dir)
    .filter((name) => name.endsWith(".input.json"))
    .sort();
  if (inputs.length === 0) {
    console.error(`no *.input.json in ${dir}`);
    process.exit(1);
  }
  const self = fileURLToPath(import.meta.url);
  for (const name of inputs) {
    execFileSync(process.execPath, [self, dir, name], { stdio: "inherit" });
  }
}
