#!/usr/bin/env bash
# Write one SHA256SUMS covering every file in a directory (no ./ prefix).

set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/generate-checksums.sh [directory]

Writes <directory>/SHA256SUMS (default directory: dist) covering every regular
file in it, excluding SHA256SUMS itself. Verify with `sha256sum -c SHA256SUMS`.
USAGE
}

case "${1:-}" in
	-h | --help)
		usage
		exit 0
		;;
esac

DIR="${1:-dist}"
if [[ ! -d "${DIR}" ]]; then
	echo "Directory not found: ${DIR}" >&2
	exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
	hash_cmd=(sha256sum --)
else
	hash_cmd=(shasum -a 256 --)
fi

cd "${DIR}"
files=()
while IFS= read -r name; do
	files+=("${name}")
done < <(find . -maxdepth 1 -type f ! -name SHA256SUMS -exec basename {} \; | LC_ALL=C sort)

if [[ ${#files[@]} -eq 0 ]]; then
	echo "No files to checksum in ${DIR}" >&2
	exit 1
fi

"${hash_cmd[@]}" "${files[@]}" > SHA256SUMS
echo "${DIR}/SHA256SUMS"
