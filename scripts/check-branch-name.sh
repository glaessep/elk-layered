#!/usr/bin/env bash
# Decides whether a branch name follows the rule <type>/<issue>-<words>.
#
# Usage: check-branch-name.sh <branch> [<remote-tip>]
#   <remote-tip>  a commit the branch already has on the remote. Leave it out for a new branch.
# Exit 0 when the branch is allowed. Exit 1, with the fix on stderr, when it is not.
#
# `main` is allowed as a name, so the very first push of a repository works; branch protection
# keeps everyone else off it. A branch already on the remote with commits beyond origin/main is
# allowed too, so a pull request branch made before this rule can still be updated.
#
# Runs on the bash 3.2 that macOS ships: no associative arrays, no case-changing expansions.
set -uo pipefail

branch="${1:-}"
tip="${2:-}"

types="feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"
good="^($types)/[0-9]+-[a-z0-9]+(-[a-z0-9]+)*$"

if [[ "$branch" =~ $good ]] || [ "$branch" = "main" ]; then
  exit 0
fi

if [ -n "$tip" ] && ! [[ "$tip" =~ ^0+$ ]] && git rev-parse -q --verify origin/main >/dev/null 2>&1; then
  if ahead="$(git rev-list --count "origin/main..$tip" 2>/dev/null)" && [ "$ahead" -gt 0 ]; then
    exit 0
  fi
fi

{
  echo "elk-layered: the branch \"$branch\" does not follow <type>/<issue>-<words>."
  echo "Rename it, then push the new name:"
  echo "  git branch -m <type>/<issue>-<words>      e.g. feat/4-ports-in-the-input"
  echo "  git push -u origin <type>/<issue>-<words>"
  echo "<type> is one of: ${types//|/, }."
  echo "<issue> is the number of the issue this branch works on."
} >&2
exit 1
