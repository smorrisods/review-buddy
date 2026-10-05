#!/usr/bin/env bash
# Build a release tarball for the review-buddy binary, its clap_mangen-
# generated man page, the bundled themes and the licence.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/release-common.sh
source "${REPO_ROOT}/scripts/lib/release-common.sh"

VERSION=""
TARGET=""
BINARY_PATH=""
MAN_DIR=""
OUTPUT_DIR="dist"
OUTPUT_PREFIX=""

usage() {
	cat <<'USAGE'
Usage: scripts/build-release-archive.sh [options]

Options:
  --version <version>         Release version or tag (for example, v0.1.0)
  --target <label>            Target label used in the file name (for example,
                              x86_64-unknown-linux-musl or macos-universal)
  --binary <path>             Built binary path
  --man-dir <path>            Directory containing the generated man page
                              (default: newest out/man next to the binary)
  --output-dir <dir>          Directory for the archive (default: dist)
  --output-prefix <prefix>    Full output path without extension; overrides
                              --output-dir and the default file name
  -h, --help                  Show this help

Writes <output-dir>/review-buddy-<version>-<target>.tar.gz and prints its path.
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
		--version) VERSION="${2:-}"; shift 2 ;;
		--target) TARGET="${2:-}"; shift 2 ;;
		--binary) BINARY_PATH="${2:-}"; shift 2 ;;
		--man-dir) MAN_DIR="${2:-}"; shift 2 ;;
		--output-dir) OUTPUT_DIR="${2:-}"; shift 2 ;;
		--output-prefix) OUTPUT_PREFIX="${2:-}"; shift 2 ;;
		-h | --help)
			usage
			exit 0
			;;
		*)
			echo "Unknown option: $1" >&2
			usage >&2
			exit 1
			;;
	esac
done

if [[ -z "${VERSION}" || -z "${BINARY_PATH}" || ( -z "${TARGET}" && -z "${OUTPUT_PREFIX}" ) ]]; then
	echo "Missing required options." >&2
	usage >&2
	exit 1
fi

if [[ ! -f "${BINARY_PATH}" ]]; then
	echo "Built binary not found at ${BINARY_PATH}" >&2
	exit 1
fi

MAN_DIR="$(resolve_man_dir "${BINARY_PATH}" "${MAN_DIR}")"

if [[ -z "${OUTPUT_PREFIX}" ]]; then
	OUTPUT_PREFIX="${OUTPUT_DIR}/review-buddy-${VERSION#v}-${TARGET}"
fi
mkdir -p "$(dirname "${OUTPUT_PREFIX}")"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

ARCHIVE_ROOT="${TMP_DIR}/review-buddy"
mkdir -p "${ARCHIVE_ROOT}/bin" "${ARCHIVE_ROOT}/share/man/man1"

install -m 0755 "${BINARY_PATH}" "${ARCHIVE_ROOT}/bin/review-buddy"
gzip -n -c "${MAN_DIR}/review-buddy.1" > "${ARCHIVE_ROOT}/share/man/man1/review-buddy.1.gz"
stage_shared_files "${REPO_ROOT}" "${ARCHIVE_ROOT}"
install -m 0644 "${REPO_ROOT}/README.md" "${ARCHIVE_ROOT}/share/doc/review-buddy/README.md"

ARCHIVE_OUTPUT="${OUTPUT_PREFIX}.tar.gz"
tar -C "${ARCHIVE_ROOT}" -czf "${ARCHIVE_OUTPUT}" .

echo "${ARCHIVE_OUTPUT}"
