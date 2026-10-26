# CLAUDE.md

Rules for any agent or human who changes this repository. [README.md](README.md) says what the
crate is and where its code comes from.

## Write in plain words

Short sentences, common words, active voice. This holds for code comments, docs, issues, pull
requests and commit messages. Write in English.

## Port, do not invent

- **Follow ELK's Java at commit `62d5909`.** That is the ELK revision elkjs 0.9.3 is built from. A
  ported function names its Java source in a comment: the file and the method.
- **elkjs 0.9.3 is the reference behaviour.** Where elkjs differs from Java on a JVM, follow
  elkjs, and say so in a comment at that spot. Examples: GWT's sort, its priority queue, float
  rounding.
- **Options keep ELK's names and defaults.** An option with no ELK counterpart does not belong
  here.
- **A step that is not ported fails with a typed error.** It never falls back to something
  that only looks like ELK's step.

## Determinism

The same graph gives the same layout, bit for bit, on every machine.

- No iteration over a `HashMap` or `HashSet` whose order can reach the result.
- No clock, no environment variable, no thread scheduling in the layout.
- Randomness only through `JavaRandom`, seeded the way ELK seeds it.

## Code

- `cargo fmt` and `cargo clippy --all-targets --all-features -- -D warnings` stay clean.
- Document every public item with a `///` comment.
- Errors use `thiserror`. New code never calls `unwrap` or `expect` on input a caller gave.
  Malformed input is an error, never a panic.
- Comments say why, not what. A comment explains a decision or a case that looks wrong but is
  not.
- Every file is under EPL-2.0. Add no other licence header.

## Tests

- **Each feature gets at least one test that shows it works.** It does not need full coverage,
  but it must fail when the feature breaks.
- Before you call a test done, break the code it guards and watch the test fail. Then restore
  the code.

```bash
cargo build
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
bash .githooks/commit-msg.test.sh
bash scripts/check-branch-name.test.sh
```

## Issues, branches, pull requests

- Every change starts from an issue in this repository. The issue says why, what, and its
  acceptance criteria.
- Branch from a freshly fetched `origin/main` as `<type>/<issue>-<words>`, for example
  `feat/4-ports-in-the-input`. `.githooks/pre-push` refuses any other name.
- One issue per branch and per pull request. The pull request body says `Closes #N`.
- CI runs on every pull request, on Ubuntu and on macOS. A pull request is green only when both
  have run and passed.
- **Agents never merge a pull request, never close an issue and never push to `main`.** The
  maintainer merges.

## Commits

Conventional Commits: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`,
`chore` or `revert`, then a scope and a summary. The footer names the issue:

```
feat(import): ports keep the side the input gives them

Refs: #4
```

`.githooks/commit-msg` deletes a `Claude-Session:` line from every message. Turn the hooks on once
per clone: `git config core.hooksPath .githooks`. A Claude Code session started in this
repository does that by itself.

## Releases

The pull request that prepares a release sets the version in `Cargo.toml`. After it merges, the
maintainer tags the merge commit `vX.Y.Z` by hand. A released tag never moves; a wrong release is
fixed by the next version. Until 1.0, a change to the public API raises the minor version.
