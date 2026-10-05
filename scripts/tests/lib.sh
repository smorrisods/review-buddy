#!/usr/bin/env bash
# Tiny assertion helpers for the script tests. Source this file.

# shellcheck disable=SC2034  # consumed by the test files that source this helper
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FAILURES=0
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "${TMP_ROOT}"' EXIT

pass() { printf 'ok   %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1"; FAILURES=$((FAILURES + 1)); }

assert() {
	local name="$1"
	shift
	if "$@" >/dev/null 2>&1; then pass "${name}"; else fail "${name}"; fi
}

assert_fails() {
	local name="$1"
	shift
	if "$@" >/dev/null 2>&1; then fail "${name}"; else pass "${name}"; fi
}

assert_eq() {
	if [[ "$2" == "$3" ]]; then pass "$1"; else fail "$1 (expected '$3', got '$2')"; fi
}

finish() {
	if [[ "${FAILURES}" -ne 0 ]]; then
		echo "${FAILURES} failure(s)"
		exit 1
	fi
}
