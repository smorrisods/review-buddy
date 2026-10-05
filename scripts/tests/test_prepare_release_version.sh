#!/usr/bin/env bash
# Exercise prepare-release-version.sh in a throwaway git repository.

set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

make_repo() {
	local repo="${TMP_ROOT}/$1"
	mkdir -p "${repo}/scripts/lib"
	cp "${REPO_ROOT}/scripts/prepare-release-version.sh" "${repo}/scripts/"
	cat > "${repo}/Cargo.toml" <<'TOML'
[workspace]
members = ["crates/a"]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
serde = { version = "0.1.0" }
TOML
	cat > "${repo}/Cargo.lock" <<'LOCK'
[[package]]
name = "other"
version = "0.1.0"

[[package]]
name = "review-buddy"
version = "0.1.0"
LOCK
	git -C "${repo}" init -q -b main
	git -C "${repo}" -c user.name=t -c user.email=t@example.com add -A
	git -C "${repo}" -c user.name=t -c user.email=t@example.com commit -q -m init
	echo "${repo}"
}

repo="$(make_repo current)"
assert_eq "current-version prints workspace version" "$("${repo}/scripts/prepare-release-version.sh" --current-version)" "0.1.0"

repo="$(make_repo dry)"
assert "dry run succeeds" "${repo}/scripts/prepare-release-version.sh" --version 0.2.0 --dry-run
assert_eq "dry run leaves tree clean" "$(git -C "${repo}" status --porcelain)" ""
assert_eq "dry run stays on main" "$(git -C "${repo}" branch --show-current)" "main"

repo="$(make_repo real)"
assert "real run succeeds" "${repo}/scripts/prepare-release-version.sh" --version v0.2.0
assert_eq "creates release branch" "$(git -C "${repo}" branch --show-current)" "chore/release-v0.2.0"
assert "bumps workspace version" grep -q '^version = "0.2.0"' "${repo}/Cargo.toml"
assert "leaves dependency version" grep -q 'serde = { version = "0.1.0" }' "${repo}/Cargo.toml"
assert_eq "bumps only own lock stanza" "$(grep -c 'version = "0.2.0"' "${repo}/Cargo.lock")" "1"
assert "lock bump is on review-buddy" grep -A1 'name = "review-buddy"' "${repo}/Cargo.lock" | grep -q '0.2.0'

repo="$(make_repo invalid)"
assert_fails "rejects non-semver" "${repo}/scripts/prepare-release-version.sh" --version 1.2
assert_fails "rejects same version" "${repo}/scripts/prepare-release-version.sh" --version 0.1.0

repo="$(make_repo dirty)"
echo "# x" >> "${repo}/Cargo.toml"
assert_fails "rejects dirty tree" "${repo}/scripts/prepare-release-version.sh" --version 0.2.0

finish
