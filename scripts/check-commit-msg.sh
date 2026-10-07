#!/usr/bin/env bash
# Checks commit subjects against Conventional Commits.
#   scripts/check-commit-msg.sh FILE         one message (commit-msg hook)
#   scripts/check-commit-msg.sh --range A..B every commit of a range (CI)
set -euo pipefail

types='feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert'
pattern="^($types)(\([a-z0-9-]+\))?!?: [^ ].{0,71}$"

check() {
  if [[ ! $1 =~ $pattern ]]; then
    echo "bad commit subject: $1" >&2
    echo "expected: type(scope)?: summary, 72 chars max, type in $types" >&2
    return 1
  fi
}

if [[ ${1:-} == --range ]]; then
  subjects=$(git log --no-merges --format=%s "$2")
  status=0
  while IFS= read -r subject; do
    [[ -z $subject ]] || check "$subject" || status=1
  done <<<"$subjects"
  exit $status
fi

check "$(head -n1 "$1")"
