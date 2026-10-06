#!/usr/bin/env bash
# Helpers shared by the release scripts. Source this file; do not execute it.
# shellcheck disable=SC2034  # variables are consumed by the scripts that source this file

PACKAGE_NAME="review-buddy"
PACKAGE_SUMMARY="Every pull and merge request, in one quiet queue"
PACKAGE_DESCRIPTION="review-buddy is a keyboard-driven terminal UI that puts every GitHub pull request and GitLab merge request in one quiet queue, with full diff review and a fully explorable offline demo mode."
PACKAGE_HOMEPAGE="https://github.com/smorrisods/review-buddy"
PACKAGE_VENDOR="Liminal HQ"
PACKAGE_CONTACT="Liminal HQ <contact@liminalhq.ca>"
PACKAGE_LICENSE="MIT"

# Collapse common architecture aliases onto the release architectures.
normalise_arch() {
	case "$1" in
		amd64 | x86_64)
			echo "amd64"
			;;
		arm64 | aarch64)
			echo "arm64"
			;;
		universal)
			echo "universal"
			;;
		*)
			echo "Unsupported architecture: $1" >&2
			return 1
			;;
	esac
}

# Modification time of $1 in epoch seconds. BSD and GNU `stat` take
# incompatible flags (and GNU's `-f` means something else), so branch on the OS.
dir_mtime() {
	if [[ "$(uname -s)" == "Darwin" ]]; then
		stat -f '%m' "$1"
	else
		stat -c '%Y' "$1"
	fi
}

# Find the newest build-script `out/man` directory under the cargo target
# directory that holds the release binary. The binary may sit in
# target/release or target/<triple>/release, and the build directory is a
# sibling of the binary's directory.
discover_man_dir() {
	local binary_path="$1"
	local release_dir dir mtime
	release_dir="$(cd "$(dirname "${binary_path}")" && pwd)"

	find "${release_dir}/build" -type d -path '*/review-buddy-*/out/man' 2>/dev/null | while IFS= read -r dir; do
		mtime="$(dir_mtime "${dir}")"
		printf '%s %s\n' "${mtime}" "${dir}"
	done | sort -rn | head -n 1 | cut -d' ' -f2-
}

# Resolve and validate the man directory; prints it on stdout.
resolve_man_dir() {
	local binary_path="$1"
	local man_dir="$2"

	if [[ -z "${man_dir}" ]]; then
		man_dir="$(discover_man_dir "${binary_path}")"
	fi
	if [[ -z "${man_dir}" || ! -d "${man_dir}" ]]; then
		echo "Generated man directory was not found." >&2
		return 1
	fi
	if [[ ! -f "${man_dir}/review-buddy.1" ]]; then
		echo "Generated man page review-buddy.1 was not found in ${man_dir}" >&2
		return 1
	fi
	printf '%s\n' "${man_dir}"
}

# Find the newest build-script `out/completions` directory, like discover_man_dir.
discover_completions_dir() {
	local binary_path="$1"
	local release_dir dir mtime
	release_dir="$(cd "$(dirname "${binary_path}")" && pwd)"

	find "${release_dir}/build" -type d -path '*/review-buddy-*/out/completions' 2>/dev/null | while IFS= read -r dir; do
		mtime="$(dir_mtime "${dir}")"
		printf '%s %s\n' "${mtime}" "${dir}"
	done | sort -rn | head -n 1 | cut -d' ' -f2-
}

# Resolve and validate the completions directory; prints it on stdout. With no
# explicit directory it uses the `completions` sibling of the man directory
# (both come from one build script), then falls back to discovery.
resolve_completions_dir() {
	local binary_path="$1"
	local completions_dir="$2"
	local man_dir="${3:-}"

	if [[ -z "${completions_dir}" && -n "${man_dir}" && -d "$(dirname "${man_dir}")/completions" ]]; then
		completions_dir="$(dirname "${man_dir}")/completions"
	fi
	if [[ -z "${completions_dir}" ]]; then
		completions_dir="$(discover_completions_dir "${binary_path}")"
	fi
	if [[ -z "${completions_dir}" || ! -d "${completions_dir}" ]]; then
		echo "Generated completions directory was not found." >&2
		return 1
	fi
	local file
	for file in review-buddy.bash _review-buddy review-buddy.fish; do
		if [[ ! -f "${completions_dir}/${file}" ]]; then
			echo "Generated completion ${file} was not found in ${completions_dir}" >&2
			return 1
		fi
	done
	printf '%s\n' "${completions_dir}"
}

# Install the bash, zsh and fish completions into a prefix-style tree.
stage_completions() {
	local completions_dir="$1"
	local root="$2"

	mkdir -p "${root}/share/bash-completion/completions" "${root}/share/zsh/site-functions" "${root}/share/fish/vendor_completions.d"
	install -m 0644 "${completions_dir}/review-buddy.bash" "${root}/share/bash-completion/completions/review-buddy"
	install -m 0644 "${completions_dir}/_review-buddy" "${root}/share/zsh/site-functions/_review-buddy"
	install -m 0644 "${completions_dir}/review-buddy.fish" "${root}/share/fish/vendor_completions.d/review-buddy.fish"
}

# Copy themes, the example config, and the licence into a prefix-style tree.
stage_shared_files() {
	local repo_root="$1"
	local root="$2"

	mkdir -p "${root}/share/review-buddy/themes" "${root}/share/doc/review-buddy"
	install -m 0644 "${repo_root}"/themes/*.toml "${root}/share/review-buddy/themes/"
	install -m 0644 "${repo_root}/config.example.toml" "${root}/share/review-buddy/config.example.toml"
	install -m 0644 "${repo_root}/LICENSE" "${root}/share/doc/review-buddy/LICENSE"
}
