#!/usr/bin/env bash
# Run every scripts/tests/test_*.sh and report a summary.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failed=0
for t in "${HERE}"/test_*.sh; do
	echo "== $(basename "${t}")"
	bash "${t}" || failed=1
done
exit "${failed}"
