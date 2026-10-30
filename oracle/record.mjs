// Lays out every `<case>.input.json` in a directory with elkjs 0.9.3 and writes what elkjs
// returns to `<case>.expected.json` beside it. `tests/elkjs_parity.rs` then holds elk-layered to
// those files, number for number.
//
//   cd oracle && npm install && node record.mjs ../tests/elkjs
//   node record.mjs ../tests/elkjs/random-group-ports --drop-crashes
//
// An elkjs exception other than ELK's `UnsupportedGraphException` stops the recording, so someone
// looks at the case. For a random corpus, `--drop-crashes` deletes such an input instead and lists
// it with its message in `dropped.txt`: there is no layout to hold elk-layered to. It does the
// same for a graph that elkjs lays out with a NaN coordinate.
//
// Record again only when a case's input changes. Never edit an expected file by hand. elkjs is
// installed here to record and nowhere else: it is never committed and never part of the crate.
//
// Each case runs in a fresh Node process. elkjs remembers what it laid out before: GWT numbers
// every object it creates from one global counter, and that number is the object's hash code. So
// the order of ELK's hash sets, and with it a layout, depends on the graphs laid out earlier in
// the same process. A fresh process gives each case the layout a caller sees on its first run.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const [, , dir, arg] = process.argv;
if (!dir) {
  console.error("usage: node record.mjs <directory of *.input.json> [--drop-crashes]");
  process.exit(2);
}
const dropCrashes = arg === "--drop-crashes";
const one = dropCrashes ? undefined : arg;
// The exit code by which the child tells the parent that elkjs crashed.
const CRASHED = 3;
// The exit code by which the child tells the parent that elkjs laid the graph out with a NaN
// coordinate. It has written the expected file by then.
const NOT_A_NUMBER = 4;

if (one) {
  // The child: lay out one case and write it.
  const { default: ELK } = await import("elkjs/lib/elk.bundled.js");
  const graph = JSON.parse(readFileSync(join(dir, one), "utf8"));
  const out = join(dir, one.replace(/\.input\.json$/, ".expected.json"));
  let laid;
  try {
    laid = await new ELK().layout(graph);
  } catch (error) {
    // elkjs refuses the graph with ELK's `UnsupportedGraphException`. The expected file says so,
    // and the parity test then expects layout() to refuse it too. Only the exception's first line
    // is kept: the rest prints GWT's object dumps, which change with every build.
    const message = String(error?.message ?? error).split("\n")[0];
    if (!message.includes("UnsupportedGraphException")) {
      // Any other exception (TypeError, stack overflow, ClassCastException...) is not a refusal.
      // Nothing is written, so someone looks at the case.
      console.error(`${one}: elkjs failed with something other than a refusal: ${message}`);
      process.exit(CRASHED);
    }
    laid = { elkjsError: message };
  }
  writeFileSync(out, JSON.stringify(laid, null, 2) + "\n");
  console.log(`recorded ${out}`);
  // JSON writes a NaN as null. A graph elkjs lays out with NaN has no layout to hold elk-layered
  // to, unless the case is on purpose: the parent decides.
  const isNaNIn = (shape) =>
    shape !== undefined &&
    ["x", "y", "width", "height"].some((key) => Number.isNaN(shape[key]));
  const pointHasNaN = (point) => point !== undefined && isNaNIn(point);
  const hasNaN = (node) =>
    isNaNIn(node) ||
    (node.labels ?? []).some(isNaNIn) ||
    (node.ports ?? []).some((port) => isNaNIn(port) || (port.labels ?? []).some(isNaNIn)) ||
    (node.edges ?? []).some(
      (edge) =>
        (edge.labels ?? []).some(isNaNIn) ||
        (edge.sections ?? []).some(
          (section) =>
            pointHasNaN(section.startPoint) ||
            pointHasNaN(section.endPoint) ||
            (section.bendPoints ?? []).some(pointHasNaN),
        ) ||
        (edge.junctionPoints ?? []).some(pointHasNaN),
    ) ||
    (node.children ?? []).some(hasNaN);
  if (hasNaN(laid)) {
    console.error(`${one}: elkjs laid it out with a NaN coordinate`);
    // Only the parent loop below reads exit code 4. Someone who records one case by hand gets
    // the warning and a success: the case may have its NaN on purpose.
    process.exit(process.env.RECORD_PARENT ? NOT_A_NUMBER : 0);
  }
} else {
  const inputs = readdirSync(dir)
    .filter((name) => name.endsWith(".input.json"))
    .sort();
  if (inputs.length === 0) {
    console.error(`no *.input.json in ${dir}`);
    process.exit(1);
  }
  const self = fileURLToPath(import.meta.url);
  const dropped = [];
  for (const name of inputs) {
    try {
      execFileSync(process.execPath, [self, dir, name], {
        stdio: ["ignore", "inherit", "pipe"],
        env: { ...process.env, RECORD_PARENT: "1" },
      });
    } catch (error) {
      const stderr = String(error.stderr ?? "").trim();
      if (error.status === NOT_A_NUMBER && !dropCrashes) {
        // Only the corpora that drop crashes also drop these: a case on purpose, such as
        // `ports-root-idle`, keeps its NaN. The warning shows, so a NaN by accident does not slip
        // into an expected file unseen.
        console.warn(`warning: ${stderr}`);
        continue;
      }
      if (error.status === NOT_A_NUMBER) {
        rmSync(join(dir, name.replace(/\.input\.json$/, ".expected.json")));
        rmSync(join(dir, name));
        dropped.push(stderr.split("\n")[0]);
        console.log(`dropped ${name}`);
        continue;
      }
      if (!dropCrashes || error.status !== CRASHED) {
        console.error(stderr);
        process.exit(1);
      }
      rmSync(join(dir, name));
      dropped.push(stderr.split("\n")[0]);
      console.log(`dropped ${name}`);
    }
  }
  if (dropCrashes) {
    const list = dropped.map((line) => `${line}\n`).join("");
    writeFileSync(join(dir, "dropped.txt"), list);
  }
}
