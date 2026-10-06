#!/bin/sh
# review-buddy installer: downloads, verifies and installs a release build.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/smorrisods/review-buddy/main/scripts/install.sh | sh
#   ./install.sh [--version vX.Y.Z] [--prefix /path] [--libc musl|glibc] [--dry-run] [--uninstall]
#
# Env vars (same effect as the matching flag): VERSION, PREFIX, NO_COLOR.
# GITHUB_TOKEN is only used, if set, to avoid API rate limits.
#
# Testing only: RB_INSTALL_BASE_URL (a directory or file:// URL holding the
# release assets and a VERSION file), RB_INSTALL_DEFAULT_PREFIX,
# RB_INSTALL_UNAME_S and RB_INSTALL_UNAME_M.
#
# POSIX sh, so it behaves the same piped from curl on Linux and macOS.

set -eu

REPO="smorrisods/review-buddy"
BINARY_NAME="review-buddy"
MANIFEST_REL="share/review-buddy/install-manifest"

VERSION="${VERSION:-}"
PREFIX="${PREFIX:-}"
LIBC="musl"
DRY_RUN=false
UNINSTALL=false

usage() {
	cat <<'USAGE'
Usage: install.sh [options]

Options:
  --version <tag>   Install a specific release tag (default: latest)
  --prefix <path>   Install prefix (default: /usr/local, or ~/.local when
                    /usr/local isn't writable and you aren't root)
  --libc <musl|glibc>
                    Linux build to install (default: musl, which is static)
  --dry-run         Show what would happen without changing anything
  --uninstall       Remove exactly what a previous install placed, then exit
  -h, --help        Show this help

Environment variables VERSION and PREFIX are equivalent to the matching
flags. Set NO_COLOR=1 to disable coloured output. The installer never runs
sudo for you; when it needs it, it prints the command instead.
USAGE
}

while [ $# -gt 0 ]; do
	case "$1" in
		--version)
			[ $# -ge 2 ] || { echo "--version needs a value" >&2; exit 1; }
			VERSION="$2"
			shift 2
			;;
		--prefix)
			[ $# -ge 2 ] || { echo "--prefix needs a value" >&2; exit 1; }
			PREFIX="$2"
			shift 2
			;;
		--libc)
			[ $# -ge 2 ] || { echo "--libc needs a value" >&2; exit 1; }
			LIBC="$2"
			shift 2
			;;
		--dry-run)
			DRY_RUN=true
			shift
			;;
		--uninstall)
			UNINSTALL=true
			shift
			;;
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

case "${LIBC}" in
	musl | glibc) ;;
	*)
		echo "Unknown --libc value '${LIBC}'. Use musl or glibc." >&2
		exit 1
		;;
esac

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
	BOLD="$(printf '\033[1m')"
	DIM="$(printf '\033[2m')"
	RESET="$(printf '\033[0m')"
	FANCY=true
else
	BOLD=""
	DIM=""
	RESET=""
	FANCY=false
fi

say() { printf '%s\n' "$*"; }
note() { printf '%s\n' "${DIM}$*${RESET}"; }
die() {
	printf 'Error: %s\n' "$*" >&2
	exit 1
}

banner() {
	if [ "${FANCY}" = true ]; then
		printf '%s\n' "${BOLD}  🦦  review buddy${RESET}"
		note "      Every pull and merge request, in one quiet queue."
		printf '\n'
	else
		say "review buddy installer"
	fi
}

fetch() {
	_url="$1"
	_dest="$2"
	case "${_url}" in
		file://*)
			cp "${_url#file://}" "${_dest}" 2>/dev/null || return 1
			;;
		*)
			if command -v curl >/dev/null 2>&1; then
				curl -fsSL "${_url}" -o "${_dest}"
			elif command -v wget >/dev/null 2>&1; then
				wget -q -O "${_dest}" "${_url}"
			else
				die "Neither curl nor wget is installed. Install one and try again."
			fi
			;;
	esac
}

latest_tag() {
	if [ -n "${RB_INSTALL_BASE_URL:-}" ]; then
		fetch "${RB_INSTALL_BASE_URL%/}/VERSION" "${WORK}/version" || die "Couldn't read VERSION from ${RB_INSTALL_BASE_URL}."
		tr -d ' \r\n' < "${WORK}/version"
		return
	fi
	_api="https://api.github.com/repos/${REPO}/releases/latest"
	if [ -n "${GITHUB_TOKEN:-}" ]; then
		if command -v curl >/dev/null 2>&1; then
			curl -fsSL -H "Authorization: Bearer ${GITHUB_TOKEN}" "${_api}" -o "${WORK}/latest.json" || return 1
		else
			wget -q --header="Authorization: Bearer ${GITHUB_TOKEN}" -O "${WORK}/latest.json" "${_api}" || return 1
		fi
	else
		fetch "${_api}" "${WORK}/latest.json" || return 1
	fi
	sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${WORK}/latest.json" | head -n 1
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{print $1}'
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | awk '{print $1}'
	else
		die "Neither sha256sum nor shasum is available, so I can't verify the download."
	fi
}

# True when we could create files under $1 (or its nearest existing parent).
can_write() {
	_dir="$1"
	while [ ! -d "${_dir}" ]; do
		_parent="$(dirname "${_dir}")"
		[ "${_parent}" = "${_dir}" ] && return 1
		_dir="${_parent}"
	done
	[ -w "${_dir}" ]
}

is_root() { [ "$(id -u)" = "0" ]; }

choose_prefix() {
	if [ -n "${PREFIX}" ]; then
		if ! is_root && ! can_write "${PREFIX}" && [ "${DRY_RUN}" = false ]; then
			say "I can't write to ${PREFIX} without elevated permissions." >&2
			say "Re-run it yourself with sudo if you want a system-wide install:" >&2
			say "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sudo sh -s -- --prefix ${PREFIX}" >&2
			say "Or pick a folder you own, for example --prefix \"\$HOME/.local\"." >&2
			exit 1
		fi
		return
	fi
	PREFIX="${RB_INSTALL_DEFAULT_PREFIX:-/usr/local}"
	if ! is_root && ! can_write "${PREFIX}"; then
		FALLBACK_FROM="${PREFIX}"
		PREFIX="${HOME}/.local"
	fi
}

remove_manifest_files() {
	_manifest="$1"
	_root="$2"
	while IFS= read -r _rel; do
		[ -n "${_rel}" ] || continue
		case "${_rel}" in
			/* | *..*) continue ;;
		esac
		if [ "${DRY_RUN}" = true ]; then
			say "  would remove ${_root}/${_rel}"
		else
			rm -f "${_root}/${_rel}"
			say "  removed ${_root}/${_rel}"
		fi
	done < "${_manifest}"
	if [ "${DRY_RUN}" = false ]; then
		rm -f "${_manifest}"
		for _d in share/review-buddy/themes share/review-buddy share/doc/review-buddy share/bash-completion/completions share/zsh/site-functions share/fish/vendor_completions.d; do
			rmdir "${_root}/${_d}" 2>/dev/null || true
		done
	fi
}

do_uninstall() {
	_root=""
	if [ -n "${PREFIX}" ]; then
		_first="${PREFIX}"
		_second="${PREFIX}"
	else
		_first="${RB_INSTALL_DEFAULT_PREFIX:-/usr/local}"
		_second="${HOME}/.local"
	fi
	for _p in "${_first}" "${_second}"; do
		if [ -f "${_p}/${MANIFEST_REL}" ]; then
			_root="${_p}"
			break
		fi
	done
	if [ -z "${_root}" ]; then
		say "I couldn't find an install manifest, so there's nothing to remove."
		say "If you installed with a package manager, uninstall it there."
		exit 0
	fi
	if [ "${DRY_RUN}" = false ] && ! is_root && [ ! -w "${_root}/${MANIFEST_REL}" ]; then
		say "I can't remove files from ${_root} without elevated permissions." >&2
		say "  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sudo sh -s -- --uninstall --prefix ${_root}" >&2
		exit 1
	fi
	say "Removing review-buddy from ${_root}"
	remove_manifest_files "${_root}/${MANIFEST_REL}" "${_root}"
	say "Done. Your config and cache are untouched."
}

detect_target() {
	_os="${RB_INSTALL_UNAME_S:-$(uname -s)}"
	_arch="${RB_INSTALL_UNAME_M:-$(uname -m)}"
	case "${_arch}" in
		x86_64 | amd64) ARCH="amd64" ;;
		aarch64 | arm64) ARCH="arm64" ;;
		*) ARCH="" ;;
	esac
	case "${_os}" in
		Linux)
			[ -n "${ARCH}" ] || die "Unsupported CPU architecture '${_arch}'. Linux builds exist for x86_64 and aarch64; see https://github.com/${REPO}/releases for what's available."
			TARGET="linux-${ARCH}-${LIBC}"
			;;
		Darwin)
			[ -n "${ARCH}" ] || die "Unsupported CPU architecture '${_arch}'. macOS builds cover x86_64 and arm64 (universal2)."
			TARGET="macos-universal2"
			;;
		*)
			die "Unsupported operating system '${_os}'. This installer covers Linux and macOS; on Windows use scripts/install.ps1."
			;;
	esac
}

banner

WORK="$(mktemp -d)"
trap 'rm -rf "${WORK}"' EXIT INT TERM
FALLBACK_FROM=""

if [ "${UNINSTALL}" = true ]; then
	do_uninstall
	exit 0
fi

choose_prefix

detect_target

if [ -z "${VERSION}" ]; then
	VERSION="$(latest_tag)" || VERSION=""
	[ -n "${VERSION}" ] || die "Couldn't work out the latest release. Check your connection, or pass --version vX.Y.Z."
fi
case "${VERSION}" in
	v*) TAG="${VERSION}" ;;
	*) TAG="v${VERSION}" ;;
esac
NUM="${TAG#v}"

ASSET="${BINARY_NAME}-${NUM}-${TARGET}.tar.gz"
if [ -n "${RB_INSTALL_BASE_URL:-}" ]; then
	BASE="${RB_INSTALL_BASE_URL%/}"
else
	BASE="https://github.com/${REPO}/releases/download/${TAG}"
fi

if [ -n "${FALLBACK_FROM}" ]; then
	note "${FALLBACK_FROM} isn't writable, so I'll install into ${PREFIX} instead."
	note "For a system-wide install, re-run with: sudo sh -s -- --prefix ${FALLBACK_FROM}"
fi

say "Installing review-buddy ${TAG} (${TARGET}) into ${PREFIX}"

if [ "${DRY_RUN}" = true ]; then
	say "Dry run: nothing will be downloaded or changed."
	say "  would download ${BASE}/${ASSET}"
	say "  would download ${BASE}/SHA256SUMS and verify the archive"
	say "  would install ${PREFIX}/bin/${BINARY_NAME}, the man page, themes and any completions"
	say "  would write ${PREFIX}/${MANIFEST_REL}"
	exit 0
fi

fetch "${BASE}/${ASSET}" "${WORK}/${ASSET}" || die "Couldn't download ${BASE}/${ASSET}. That build may not exist for this release; see https://github.com/${REPO}/releases."
fetch "${BASE}/SHA256SUMS" "${WORK}/SHA256SUMS" || die "Couldn't download SHA256SUMS, so I can't verify the archive."

expected="$(awk -v n="${ASSET}" '$2 == n || $2 == "*" n { print $1 }' "${WORK}/SHA256SUMS" | head -n 1)"
[ -n "${expected}" ] || die "SHA256SUMS has no entry for ${ASSET}. Nothing was installed."
actual="$(sha256_of "${WORK}/${ASSET}")"
if [ "${expected}" != "${actual}" ]; then
	die "Checksum mismatch for ${ASSET} (expected ${expected}, got ${actual}). Nothing was installed."
fi
say "Checksum verified."

STAGE="${WORK}/stage"
mkdir -p "${STAGE}"
tar -xzf "${WORK}/${ASSET}" -C "${STAGE}"
[ -f "${STAGE}/bin/${BINARY_NAME}" ] || die "The archive has no bin/${BINARY_NAME}. Nothing was installed."

mkdir -p "${PREFIX}"
LIST="${WORK}/files"
(cd "${STAGE}" && find . -type f | sed 's|^\./||' | LC_ALL=C sort) > "${LIST}"

# Remove files from an earlier install so a re-install leaves no strays.
if [ -f "${PREFIX}/${MANIFEST_REL}" ]; then
	remove_manifest_files "${PREFIX}/${MANIFEST_REL}" "${PREFIX}" >/dev/null
fi

while IFS= read -r rel; do
	mkdir -p "${PREFIX}/$(dirname "${rel}")"
	cp "${STAGE}/${rel}" "${PREFIX}/${rel}"
	if [ "${rel}" = "bin/${BINARY_NAME}" ]; then
		chmod 0755 "${PREFIX}/${rel}"
	else
		chmod 0644 "${PREFIX}/${rel}"
	fi
done < "${LIST}"

mkdir -p "${PREFIX}/share/review-buddy"
cp "${LIST}" "${PREFIX}/${MANIFEST_REL}"
echo "${MANIFEST_REL}" >> "${PREFIX}/${MANIFEST_REL}"

say "Installed $(wc -l < "${LIST}" | tr -d ' ') files."
case ":${PATH}:" in
	*":${PREFIX}/bin:"*) ;;
	*) say "Note: ${PREFIX}/bin isn't on your PATH yet. Add it to your shell profile to run review-buddy directly." ;;
esac

printf '\n'
say "Next steps"
say "  ${BINARY_NAME} --demo         explore with no network or account"
say "  ${BINARY_NAME} auth status    check your GitHub and GitLab sign-in"
say "To remove it later, run this installer again with --uninstall."
