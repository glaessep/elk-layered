// Writes the random parity corpus: 200 small ELK graphs as elkjs JSON in ../tests/elkjs/random.
// `record.mjs` then records what elkjs 0.9.3 returns for each, and `tests/elkjs_parity.rs` holds
// `layout()` to it.
//
//   cd oracle && npm install && node random.mjs ../tests/elkjs/random && node record.mjs ../tests/elkjs/random
//   node random.mjs ../tests/elkjs/random-group-ends --group-ends && node record.mjs ../tests/elkjs/random-group-ends
//
// With `--group-ends`, an edge may also start or end at a group, not only at a leaf. elkjs
// refuses some of those graphs; record.mjs then records the refusal.
//
//   node random.mjs ../tests/elkjs/random-ports --ports && node record.mjs ../tests/elkjs/random-ports
//
// With `--ports`, the graphs have no groups, and many leaves have ports, under the port
// constraints FREE, FIXED_SIDE or FIXED_ORDER. A port of a fixed node sits on a side along the
// layout flow: west or east for the directions none, RIGHT and LEFT, north or south for DOWN and
// UP. An edge may start or end at a port.
//
//   node random.mjs ../tests/elkjs/random-group-ports --group-ports && node record.mjs ../tests/elkjs/random-group-ports --drop-crashes
//
// With `--group-ports`, groups have ports too, under SEPARATE_CHILDREN and INCLUDE_CHILDREN. The
// side of a port follows the direction of the run that lays its node out. A group under
// SEPARATE_CHILDREN sets no direction, so its children and its nested run use RIGHT.
//
// The seed is fixed, so the same command writes the same files. Change the generator only to add
// a feature, and then write and record the whole corpus again.
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node random.mjs <directory> [--group-ends | --ports]");
  process.exit(2);
}
const mode = process.argv[3] ?? "";
const groupEnds = mode === "--group-ends";
const groupPorts = mode === "--group-ports";
const withPorts = mode === "--ports" || groupPorts;
const COUNT = { "--group-ends": 120, "--ports": 150, "--group-ports": 150 }[mode] ?? 200;
const SEED = { "--group-ends": 0x6eed, "--ports": 0x7eed, "--group-ports": 0x8eed }[mode] ?? 0x5eed;

// mulberry32: a small generator with a fixed seed. Its numbers only pick the graphs, so any
// generator with a fixed seed would do.
function generator(seed) {
  let state = seed >>> 0;
  const next = () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const int = (lo, hi) => lo + Math.floor(next() * (hi - lo + 1));
  const chance = (p) => next() < p;
  const pick = (items) => items[int(0, items.length - 1)];
  return { int, chance, pick };
}

const rand = generator(SEED);

// Sizes on a half-pixel grid, and now and then on a tenth, which no binary fraction holds exactly.
function size(lo, hi) {
  if (rand.chance(0.1)) return lo + rand.int(0, (hi - lo) * 10) / 10;
  return lo + rand.int(0, (hi - lo) * 2) / 2;
}

function label(text) {
  return { text, width: Math.round(6.5 * text.length * 10) / 10, height: 14 };
}

const WORDS = ["req", "ack", "x", "speed", "0..*", "1", "pedal %", "brake request", "on"];

// elkjs gives back every field of the input, an empty `edges` list too, and elk-layered writes no
// empty list. So the generator writes none either: a list appears only with something in it.
function graph() {
  const options = { "elk.algorithm": "layered" };
  const direction = rand.pick([null, "RIGHT", "LEFT", "DOWN", "UP"]);
  if (direction) options["elk.direction"] = direction;
  const hierarchy = groupPorts
    ? rand.pick(["include", "include", "separate"])
    : withPorts
      ? "flat"
      : rand.pick(["flat", "flat", "include", "include", "separate"]);
  if (hierarchy === "include") options["elk.hierarchyHandling"] = "INCLUDE_CHILDREN";
  if (rand.chance(0.5)) options["elk.layered.considerModelOrder.strategy"] = "NODES_AND_EDGES";
  const seed = rand.pick([null, null, "1", String(rand.int(2, 1000000))]);
  if (seed) options["elk.randomSeed"] = seed;
  if (rand.chance(0.2)) options["elk.spacing.nodeNode"] = String(rand.int(5, 40));
  if (rand.chance(0.2)) options["elk.layered.spacing.nodeNodeBetweenLayers"] = String(rand.int(10, 60));
  if (rand.chance(0.15)) options["elk.separateConnectedComponents"] = "false";

  const root = { id: "root", layoutOptions: options, children: [] };
  const leaves = [];
  const groups = [];
  const parent = new Map();

  // Groups first, nested at most two deep, then leaves spread over the root and the groups.
  const containers = [root];
  if (hierarchy !== "flat") {
    for (let g = 0, n = rand.int(1, 3); g < n; g++) {
      const into = rand.pick(containers);
      const group = { id: `g${g}`, children: [] };
      if (rand.chance(0.4)) {
        group.labels = [label(rand.pick(["Controller", "Brake System Controller", "IO"]))];
        group.layoutOptions = {
          "elk.nodeLabels.placement": "[H_LEFT, V_TOP, INSIDE]",
          "elk.nodeSize.constraints": "[NODE_LABELS]",
        };
      }
      into.children.push(group);
      parent.set(group.id, into);
      groups.push(group);
      if (into === root) containers.push(group);
    }
  }
  for (let n = 0, count = rand.int(3, 15); n < count; n++) {
    const into = groups.length > 0 && rand.chance(0.6) ? rand.pick(groups) : root;
    const node = { id: `n${n}`, width: size(20, 120), height: size(20, 60) };
    if (rand.chance(0.15)) {
      node.labels = [label(rand.pick(["A", "Sensor", "ECU"]))];
      node.layoutOptions = { "elk.nodeLabels.placement": "[H_CENTER, V_CENTER, INSIDE]" };
    }
    into.children.push(node);
    parent.set(node.id, into);
    leaves.push(node);
  }
  // A group with nothing inside is a plain node to ELK; give each one a leaf so it stays a group.
  for (const group of groups) {
    if (group.children.length === 0) {
      const node = { id: `n${leaves.length}`, width: size(20, 120), height: size(20, 60) };
      group.children.push(node);
      parent.set(node.id, group);
      leaves.push(node);
    }
  }

  const ancestors = (id) => {
    const chain = [];
    for (let at = parent.get(id); at; at = parent.get(at.id)) chain.push(at);
    return chain;
  };
  // The deepest node that holds both ends: where ELK keeps an edge between them.
  const common = (a, b) => {
    const outer = new Set(ancestors(b));
    return ancestors(a).find((c) => outer.has(c)) ?? root;
  };

  const ends = leaves.map((n) => n.id);
  if (groupEnds) ends.push(...groups.map((g) => g.id));
  if (withPorts) {
    // Along the flow, so that no port needs the north and south port steps of layered. The flow
    // is the direction of the run that lays the node out. That is the root's direction for the
    // root's children and for everything under INCLUDE_CHILDREN. A group under
    // SEPARATE_CHILDREN sets no direction, so the run of its children uses the default, RIGHT.
    const sidesOf = (node) => {
      const own = parent.get(node.id) === root || hierarchy === "include" ? direction : null;
      return own === "DOWN" || own === "UP" ? ["NORTH", "SOUTH"] : ["WEST", "EAST"];
    };
    let p = 0;
    for (const node of groupPorts ? [...leaves, ...groups] : leaves) {
      const flowSides = sidesOf(node);
      if (!rand.chance(0.6)) continue;
      const constraint = rand.pick([null, "FIXED_SIDE", "FIXED_SIDE", "FIXED_ORDER"]);
      if (constraint) (node.layoutOptions ??= {})["elk.portConstraints"] = constraint;
      node.ports = [];
      for (let i = 0, n = rand.int(1, 4); i < n; i++) {
        const port = { id: `p${p++}`, width: rand.int(4, 10), height: rand.int(4, 10) };
        if (constraint) port.layoutOptions = { "elk.port.side": rand.pick(flowSides) };
        node.ports.push(port);
        parent.set(port.id, parent.get(node.id));
        ends.push(port.id);
      }
    }
  }
  let e = 0;
  const addEdge = (source, target) => {
    const edge = { id: `e${e++}`, sources: [source], targets: [target] };
    if (rand.chance(0.25)) {
      edge.labels = [];
      for (let l = 0, n = rand.int(1, 2); l < n; l++) {
        const lab = label(rand.pick(WORDS));
        const placement = rand.pick([null, "CENTER", "HEAD", "TAIL"]);
        if (placement) lab.layoutOptions = { "elk.edgeLabels.placement": placement };
        edge.labels.push(lab);
      }
    }
    // elkjs 0.9.3 crashes ("Cannot read properties of null (reading 'd')") on a node that has a
    // loop at one port with a tail and a head label, and another edge with a head label at a
    // second port. The labels are drawn and then dropped, so the random draws stay the same.
    if (withPorts && source === target && source.startsWith("p")) delete edge.labels;
    // Most edges sit in the node that holds both ends; some sit in the root, as many callers
    // write them, and ELK moves them.
    const into = rand.chance(0.3) ? root : common(source, target);
    (into.edges ??= []).push(edge);
  };
  for (let m = 0, count = rand.int(ends.length - 1, 2 * ends.length); m < count; m++) {
    const source = rand.pick(ends);
    const target = rand.pick(ends);
    if (source === target && !rand.chance(0.3)) continue;
    addEdge(source, target);
    if (rand.chance(0.08)) addEdge(source, target);
  }
  return root;
}

mkdirSync(dir, { recursive: true });
for (const name of readdirSync(dir)) {
  if (name.endsWith(".input.json") || name.endsWith(".expected.json")) rmSync(join(dir, name));
}
for (let i = 0; i < COUNT; i++) {
  const name = `r${String(i).padStart(3, "0")}.input.json`;
  writeFileSync(join(dir, name), JSON.stringify(graph(), null, 2) + "\n");
}
console.log(`wrote ${COUNT} graphs to ${dir}`);
