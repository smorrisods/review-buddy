#!/usr/bin/env bash
# Update release-facing version references before tagging a new review-buddy
# release.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RED=""
GREEN=""
YELLOW=""
BLUE=""
BOLD=""
RESET=""

if [[ -t 1 ]]; then
	RED="$(printf '\033[31m')"
	GREEN="$(printf '\033[32m')"
	YELLOW="$(printf '\033[33m')"
	BLUE="$(printf '\033[34m')"
	BOLD="$(printf '\033[1m')"
	RESET="$(printf '\033[0m')"
fi

usage() {
	cat <<'USAGE'
Usage:
  scripts/prepare-release-version.sh --current-version
  scripts/prepare-release-version.sh --version <version> [--branch <name>] [--dry-run]

Options:
  --current-version     Print the current package version and exit
  --version <version>   New release version, with or without a leading `v`
  --branch <name>       Branch to create before updating files
  --dry-run             Show planned changes without writing files
  -h, --help             Show this help

This script updates release-facing version references and prepares the repo
for review before a release tag is created on `main`.

Examples:
  scripts/prepare-release-version.sh --current-version
  scripts/prepare-release-version.sh --version 0.2.0
  scripts/prepare-release-version.sh --version 0.2.0 --branch chore/my-release-branch
  scripts/prepare-release-version.sh --version 0.2.0 --dry-run
USAGE
}

info() {
	printf '%b\n' "${BLUE}${1}${RESET}"
}

success() {
	printf '%b\n' "${GREEN}${1}${RESET}"
}

warn() {
	printf '%b\n' "${YELLOW}${1}${RESET}"
}

fail() {
	printf '%b\n' "${RED}${1}${RESET}" >&2
	exit 1
}

usage_error() {
	printf '%b\n\n' "${RED}${1}${RESET}" >&2
	usage >&2
	exit 1
}

require_clean_repo() {
	if ! git -C "${REPO_ROOT}" diff --quiet || ! git -C "${REPO_ROOT}" diff --cached --quiet; then
		fail "Working tree has tracked changes. Commit or stash them before running this script."
	fi
}

current_branch() {
	git -C "${REPO_ROOT}" branch --show-current
}

# The version lives in the root Cargo.toml `[workspace.package]` table.
current_package_version() {
	awk '/^\[/ { in_pkg = ($0 == "[workspace.package]") } in_pkg && /^version = / { gsub(/^version = "|"$/, ""); print; exit }' "${REPO_ROOT}/Cargo.toml"
}

# Bump the workspace version in the root Cargo.toml `[workspace.package]` table.
bump_workspace_version() {
	local file="$1"
	local current="$2"
	local new="$3"

	perl -0pi -e "s/(\\[workspace\\.package\\][^\\[]*?\\nversion = \")\\Q${current}\\E(\")/\${1}${new}\${2}/" "${file}"

	if [[ "$(current_package_version)" != "${new}" ]]; then
		fail "Failed to update [workspace.package] version in ${file#"${REPO_ROOT}"/}. File layout may have changed; update this script."
	fi
}

# Bump only *our own* `name = "review-buddy"` stanza in Cargo.lock, not any
# dependency pinned to the same version string. Fails loudly if the stanza
# isn't found, since `perl -pi` exits 0 even on zero matches.
bump_lock_version() {
	local file="$1"
	local current="$2"
	local new="$3"

	perl -0pi -e "s/(name = \"review-buddy\"\nversion = \")\\Q${current}\\E(\")/\${1}${new}\${2}/" "${file}"

	if ! grep -A1 'name = "review-buddy"' "${file}" | grep -q "version = \"${new}\""; then
		fail "Failed to update review-buddy's version stanza in ${file#"${REPO_ROOT}"/}. Expected \`name = \"review-buddy\"\` immediately followed by \`version = \"${new}\"\`."
	fi
}

VERSION_INPUT=""
BRANCH_INPUT=""
SHOW_CURRENT_VERSION=false
DRY_RUN=false

while [[ $# -gt 0 ]]; do
	case "$1" in
		--current-version)
			SHOW_CURRENT_VERSION=true
			shift
			;;
		--version)
			if [[ $# -lt 2 || -z "${2:-}" ]]; then
				usage_error "Missing value for --version"
			fi
			VERSION_INPUT="${2}"
			shift 2
			;;
		--branch)
			if [[ $# -lt 2 || -z "${2:-}" ]]; then
				usage_error "Missing value for --branch"
			fi
			BRANCH_INPUT="${2}"
			shift 2
			;;
		--dry-run)
			DRY_RUN=true
			shift
			;;
		-h | --help)
			usage
			exit 0
			;;
		*)
			usage_error "Unknown option: $1"
			;;
	esac
done

if [[ "${SHOW_CURRENT_VERSION}" == true ]]; then
	if [[ -n "${VERSION_INPUT}" || -n "${BRANCH_INPUT}" || "${DRY_RUN}" == true ]]; then
		usage_error "--current-version cannot be combined with other options"
	fi

	CURRENT_VERSION="$(current_package_version)"
	if [[ -z "${CURRENT_VERSION}" ]]; then
		fail "Could not determine the current package version from Cargo.toml"
	fi

	printf '%s\n' "${CURRENT_VERSION}"
	exit 0
fi

if [[ -z "${VERSION_INPUT}" ]]; then
	usage_error "Missing required option: --version or --current-version"
fi

if [[ ! "${VERSION_INPUT}" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
	usage_error "Version must look like 0.2.0 or v0.2.0"
fi

require_clean_repo

CURRENT_VERSION="$(current_package_version)"
if [[ -z "${CURRENT_VERSION}" ]]; then
	fail "Could not determine the current package version from Cargo.toml"
fi

NEW_VERSION="${VERSION_INPUT#v}"
NEW_TAG="v${NEW_VERSION}"
TARGET_BRANCH="${BRANCH_INPUT:-chore/release-${NEW_TAG}}"

if [[ "${CURRENT_VERSION}" == "${NEW_VERSION}" ]]; then
	fail "Version is already ${NEW_VERSION}"
fi

FILES=(
	"${REPO_ROOT}/Cargo.toml"
	"${REPO_ROOT}/Cargo.lock"
)

info "${BOLD}Preparing release version bump${RESET}"
printf '  from %b%s%b to %b%s%b\n' "${YELLOW}" "${CURRENT_VERSION}" "${RESET}" "${GREEN}" "${NEW_VERSION}" "${RESET}"
printf '  on branch %b%s%b\n' "${GREEN}" "${TARGET_BRANCH}" "${RESET}"

for file in "${FILES[@]}"; do
	if [[ ! -f "${file}" ]]; then
		fail "Expected file not found: ${file}"
	fi
done

if [[ "${DRY_RUN}" == true ]]; then
	warn "Dry run only. No files will be changed."
	printf '  would create or reuse branch %s\n' "${TARGET_BRANCH}"
	for file in "${FILES[@]}"; do
		printf '  would update %s\n' "${file#"${REPO_ROOT}"/}"
	done
	exit 0
fi

CURRENT_BRANCH_NAME="$(current_branch)"
if [[ "${CURRENT_BRANCH_NAME}" != "${TARGET_BRANCH}" ]]; then
	if git -C "${REPO_ROOT}" show-ref --verify --quiet "refs/heads/${TARGET_BRANCH}"; then
		fail "Branch already exists locally: ${TARGET_BRANCH}"
	fi

	info "Creating branch ${TARGET_BRANCH}"
	git -C "${REPO_ROOT}" checkout -b "${TARGET_BRANCH}" >/dev/null
fi

bump_workspace_version "${REPO_ROOT}/Cargo.toml" "${CURRENT_VERSION}" "${NEW_VERSION}"
# Cargo.lock records our own package's version in its `[[package]] name = "review-buddy"` stanza;
# bump only that occurrence (not any same-numbered dependency version).
bump_lock_version "${REPO_ROOT}/Cargo.lock" "${CURRENT_VERSION}" "${NEW_VERSION}"

success "Updated release version references in:"
for file in "${FILES[@]}"; do
	printf '  %b- %s%b\n' "${GREEN}" "${file#"${REPO_ROOT}"/}" "${RESET}"
done

warn "Next steps:"
printf '  1. review the diff\n'
printf '  2. run cargo checks (fmt, clippy, nextest)\n'
printf '  3. commit and open a PR from %b%s%b\n' "${BOLD}" "${TARGET_BRANCH}" "${RESET}"
printf '  4. merge the PR to main\n'
printf '  5. create tag %b%s%b on main\n' "${BOLD}" "${NEW_TAG}" "${RESET}"
