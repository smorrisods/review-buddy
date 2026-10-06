#!/usr/bin/env bash
# Exercise the archive and checksum scripts with a fake binary and man page.

set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

target_dir="${TMP_ROOT}/target/x86_64-unknown-linux-musl/release"
mkdir -p "${target_dir}/build/review-buddy-abc/out/man"
printf '#!/bin/sh\necho fake\n' > "${target_dir}/review-buddy"
chmod +x "${target_dir}/review-buddy"
printf '.TH REVIEW-BUDDY 1\n' > "${target_dir}/build/review-buddy-abc/out/man/review-buddy.1"
mkdir -p "${target_dir}/build/review-buddy-abc/out/completions"
for f in review-buddy.bash _review-buddy review-buddy.fish review-buddy.elv _review-buddy.ps1; do
	printf '# fake %s\n' "${f}" > "${target_dir}/build/review-buddy-abc/out/completions/${f}"
done

dist="${TMP_ROOT}/dist"
out="$("${REPO_ROOT}/scripts/build-release-archive.sh" --version v0.1.0 --target x86_64-unknown-linux-musl --binary "${target_dir}/review-buddy" --output-dir "${dist}")"
assert_eq "archive path uses version and target" "${out}" "${dist}/review-buddy-0.1.0-x86_64-unknown-linux-musl.tar.gz"

listing="$(tar -tzf "${out}")"
for entry in ./bin/review-buddy ./share/man/man1/review-buddy.1.gz ./share/bash-completion/completions/review-buddy ./share/zsh/site-functions/_review-buddy ./share/fish/vendor_completions.d/review-buddy.fish ./share/doc/review-buddy/LICENSE ./share/doc/review-buddy/README.md ./share/review-buddy/config.example.toml ./share/review-buddy/themes/dusk.toml; do
	assert "archive contains ${entry}" grep -qx "${entry}" <<< "${listing}"
done

extract="${TMP_ROOT}/extract"
mkdir -p "${extract}"
tar -xzf "${out}" -C "${extract}"
assert "binary is executable" test -x "${extract}/bin/review-buddy"
assert_eq "man page is gzip of source" "$(gzip -dc "${extract}/share/man/man1/review-buddy.1.gz")" ".TH REVIEW-BUDDY 1"

assert_eq "bash completion is staged" "$(cat "${extract}/share/bash-completion/completions/review-buddy")" "# fake review-buddy.bash"
assert_eq "zsh completion is staged" "$(cat "${extract}/share/zsh/site-functions/_review-buddy")" "# fake _review-buddy"

assert_fails "missing completions fail" "${REPO_ROOT}/scripts/build-release-archive.sh" --version v0.1.0 --target t --binary "${target_dir}/review-buddy" --completions-dir "${TMP_ROOT}/none" --output-dir "${dist}"
assert_fails "missing binary fails" "${REPO_ROOT}/scripts/build-release-archive.sh" --version v0.1.0 --target t --binary "${TMP_ROOT}/nope" --output-dir "${dist}"
assert_fails "missing man page fails" "${REPO_ROOT}/scripts/build-release-archive.sh" --version v0.1.0 --target t --binary "${target_dir}/review-buddy" --man-dir "${TMP_ROOT}/none" --output-dir "${dist}"

if command -v dpkg-deb >/dev/null 2>&1; then
	deb="$("${REPO_ROOT}/scripts/build-linux-packages.sh" --version v0.1.0 --arch arm64 --binary "${target_dir}/review-buddy" --libc musl --format deb --output-prefix "${dist}/review-buddy-0.1.0-linux-arm64")"
	control="$(dpkg-deb -f "${deb}")"
	assert "deb has arm64 architecture" grep -qx 'Architecture: arm64' <<< "${control}"
	assert "deb has maintainer" grep -qx 'Maintainer: Liminal HQ <contact@liminalhq.ca>' <<< "${control}"
	assert "static deb has no libc6 dependency" bash -c '! grep -q "^Depends:" <<< "$1"' _ "${control}"
	assert "deb ships binary" bash -c 'dpkg-deb -c "$1" | grep -q "usr/bin/review-buddy"' _ "${deb}"
	assert "deb ships bash completion" bash -c 'dpkg-deb -c "$1" | grep -q "usr/share/bash-completion/completions/review-buddy"' _ "${deb}"
	assert "deb ships zsh completion" bash -c 'dpkg-deb -c "$1" | grep -q "usr/share/zsh/site-functions/_review-buddy"' _ "${deb}"
	assert "deb ships fish completion" bash -c 'dpkg-deb -c "$1" | grep -q "usr/share/fish/vendor_completions.d/review-buddy.fish"' _ "${deb}"
	assert "deb ships man page" bash -c 'dpkg-deb -c "$1" | grep -q "usr/share/man/man1/review-buddy.1.gz"' _ "${deb}"
fi

rm -f "${dist}"/*.deb
(cd "${dist}" && printf 'a' > a.bin)
sums="$("${REPO_ROOT}/scripts/generate-checksums.sh" "${dist}")"
assert_eq "checksum path" "${sums}" "${dist}/SHA256SUMS"
assert "SHA256SUMS verifies" bash -c 'cd "$1" && sha256sum -c SHA256SUMS' _ "${dist}"
assert "SHA256SUMS has no ./ prefix" bash -c '! grep -q "\./" "$1/SHA256SUMS"' _ "${dist}"
assert "SHA256SUMS excludes itself" bash -c '! grep -q " SHA256SUMS$" "$1/SHA256SUMS"' _ "${dist}"
assert_eq "SHA256SUMS lists every artefact" "$(wc -l < "${dist}/SHA256SUMS" | tr -d ' ')" "2"

finish
