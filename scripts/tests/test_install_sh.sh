#!/usr/bin/env bash
# Exercise scripts/install.sh against a local fake release (file:// base URL).

set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

INSTALL="${REPO_ROOT}/scripts/install.sh"
export NO_COLOR=1

rel="${TMP_ROOT}/release"
src="${TMP_ROOT}/src"
mkdir -p "${rel}" "${src}/bin" "${src}/share/man/man1" "${src}/share/review-buddy/themes"
printf '#!/bin/sh\necho fake\n' > "${src}/bin/review-buddy"
chmod +x "${src}/bin/review-buddy"
printf '.TH X 1\n' | gzip -n > "${src}/share/man/man1/review-buddy.1.gz"
printf 'name = "dusk"\n' > "${src}/share/review-buddy/themes/dusk.toml"
asset="review-buddy-0.1.0-linux-amd64-musl.tar.gz"
tar -C "${src}" -czf "${rel}/${asset}" .
printf 'v0.1.0\n' > "${rel}/VERSION"
(cd "${rel}" && sha256sum "${asset}" > SHA256SUMS)

export RB_INSTALL_BASE_URL="file://${rel}"
export RB_INSTALL_UNAME_S=Linux RB_INSTALL_UNAME_M=x86_64
export HOME="${TMP_ROOT}/home"
mkdir -p "${HOME}"

# checksum success
p1="${TMP_ROOT}/p1"
assert "install succeeds with a valid checksum" sh "${INSTALL}" --prefix "${p1}"
assert "binary installed" test -x "${p1}/bin/review-buddy"
assert "man page installed" test -f "${p1}/share/man/man1/review-buddy.1.gz"
assert "theme installed" test -f "${p1}/share/review-buddy/themes/dusk.toml"
assert "manifest written" test -f "${p1}/share/review-buddy/install-manifest"

# uninstall removes only what was installed
mkdir -p "${p1}/share/other"
printf 'keep' > "${p1}/share/other/keep.txt"
printf 'keep' > "${p1}/bin/other-tool"
assert "uninstall succeeds" sh "${INSTALL}" --uninstall --prefix "${p1}"
assert "binary removed" test ! -e "${p1}/bin/review-buddy"
assert "manifest removed" test ! -e "${p1}/share/review-buddy/install-manifest"
assert "theme dir removed" test ! -e "${p1}/share/review-buddy"
assert "unrelated file kept" test -f "${p1}/bin/other-tool"
assert "unrelated share file kept" test -f "${p1}/share/other/keep.txt"
assert "uninstall without manifest is calm" sh "${INSTALL}" --uninstall --prefix "${p1}"

# dry run touches nothing
p2="${TMP_ROOT}/p2"
assert "dry run succeeds" sh "${INSTALL}" --dry-run --prefix "${p2}"
assert "dry run creates nothing" test ! -e "${p2}"

# checksum mismatch is refused
bad="${TMP_ROOT}/bad"
cp -R "${rel}" "${bad}"
printf '0000000000000000000000000000000000000000000000000000000000000000  %s\n' "${asset}" > "${bad}/SHA256SUMS"
p3="${TMP_ROOT}/p3"
out="$(RB_INSTALL_BASE_URL="file://${bad}" sh "${INSTALL}" --prefix "${p3}" 2>&1)"
status=$?
assert_eq "mismatch exits non-zero" "$([ "${status}" -ne 0 ] && echo yes)" "yes"
assert "mismatch explains itself" grep -q "Checksum mismatch" <<< "${out}"
assert "mismatch installs nothing" test ! -e "${p3}/bin/review-buddy"

# missing entry in SHA256SUMS is refused
: > "${bad}/SHA256SUMS"
assert_fails "missing checksum entry refused" env RB_INSTALL_BASE_URL="file://${bad}" sh "${INSTALL}" --prefix "${p3}"

# default prefix falls back to ~/.local when not writable
if [ "$(id -u)" != "0" ]; then
	ro="${TMP_ROOT}/ro"
	mkdir -p "${ro}"
	chmod 0555 "${ro}"
	out="$(RB_INSTALL_DEFAULT_PREFIX="${ro}" sh "${INSTALL}" 2>&1)"
	assert "fallback installs under ~/.local" test -x "${HOME}/.local/bin/review-buddy"
	assert "fallback note is shown" grep -q "isn't writable" <<< "${out}"
	assert "fallback leaves default untouched" test ! -e "${ro}/bin"
	out="$(sh "${INSTALL}" --prefix "${ro}/sub" 2>&1)"
	assert "explicit unwritable prefix prints sudo command" grep -q "sudo sh -s -- --prefix" <<< "${out}"
	RB_INSTALL_DEFAULT_PREFIX="${ro}" sh "${INSTALL}" --uninstall >/dev/null 2>&1
	assert "uninstall finds the ~/.local install" test ! -e "${HOME}/.local/bin/review-buddy"
	chmod 0755 "${ro}"
fi

# unsupported platforms
out="$(RB_INSTALL_UNAME_S=FreeBSD sh "${INSTALL}" --prefix "${TMP_ROOT}/p4" 2>&1)"
assert "unsupported OS copy" grep -q "Unsupported operating system 'FreeBSD'" <<< "${out}"
out="$(RB_INSTALL_UNAME_M=riscv64 sh "${INSTALL}" --prefix "${TMP_ROOT}/p4" 2>&1)"
assert "unsupported arch copy" grep -q "Unsupported CPU architecture 'riscv64'" <<< "${out}"
assert_fails "bad libc rejected" sh "${INSTALL}" --libc uclibc --prefix "${TMP_ROOT}/p4"
assert "help works" sh "${INSTALL}" --help

finish
