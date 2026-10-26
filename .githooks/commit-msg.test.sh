#!/usr/bin/env bash
# Checks `commit-msg`: the lines it deletes, and the lines it must leave alone. It runs the hook
# the way git does, with one argument: the path to the message file.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
hook="$here/commit-msg"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
failures=0

# one_case NAME INPUT EXPECTED
one_case() {
  local name="$1" input="$2" expected="$3" file="$scratch/msg"
  printf '%s' "$input" > "$file"
  if ! bash "$hook" "$file"; then
    echo "FAIL $name: the hook exited non-zero, so git would reject the commit"
    failures=$((failures + 1))
    return
  fi
  # Command substitution drops trailing newlines, so EXPECTED is written without them.
  local got
  got="$(cat "$file")"
  if [ "$got" != "$expected" ]; then
    echo "FAIL $name"
    echo "  expected: $(printf '%q' "$expected")"
    echo "  got:      $(printf '%q' "$got")"
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

one_case "deletes the session trailer" \
  $'fix: a\n\nRefs: #4\nClaude-Session: https://claude.ai/code/session_1\n' \
  $'fix: a\n\nRefs: #4'
one_case "deletes it whatever its case" \
  $'fix: a\n\nclaude-session: https://claude.ai/code/session_1\n' \
  $'fix: a'
one_case "keeps a subject that names claude.ai" \
  $'fix: handle claude.ai links in labels\n' \
  $'fix: handle claude.ai links in labels'
one_case "keeps a body line that mentions the trailer mid-line" \
  $'docs: a\n\nThe hook deletes a Claude-Session: line.\n' \
  $'docs: a\n\nThe hook deletes a Claude-Session: line.'
one_case "keeps a co-author line" \
  $'feat: a\n\nCo-Authored-By: Someone <someone@example.invalid>\n' \
  $'feat: a\n\nCo-Authored-By: Someone <someone@example.invalid>'
one_case "a message that is only the trailer becomes empty" \
  $'Claude-Session: https://claude.ai/code/session_1\n' \
  ''

# An unreadable file: the hook must leave it alone and still exit 0.
missing="$scratch/does-not-exist"
if bash "$hook" "$missing" 2>/dev/null; then
  echo "ok   an unreadable message file is left alone"
else
  echo "FAIL an unreadable message file made the hook exit non-zero"
  failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases passed"
