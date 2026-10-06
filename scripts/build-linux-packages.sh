#!/usr/bin/env bash
# Build Linux release packages (.deb and .rpm) for the review-buddy binary,
# its clap_mangen-generated man page, shell completions and the bundled themes.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/lib/release-common.sh
source "${REPO_ROOT}/scripts/lib/release-common.sh"

VERSION=""
ARCH_INPUT=""
BINARY_PATH=""
MAN_DIR=""
COMPLETIONS_DIR=""
OUTPUT_PREFIX=""
FORMAT="all"
LIBC="gnu"

usage() {
	cat <<'USAGE'
Usage: scripts/build-linux-packages.sh [options]

Options:
  --version <version>         Package version or tag (for example, v0.1.0)
  --arch <amd64|arm64>        Target architecture
  --binary <path>             Built binary path
  --man-dir <path>            Directory containing the generated man page
  --completions-dir <path>    Directory containing the generated completions
                              (default: next to the man directory, else
                              discovered next to the binary)
  --output-prefix <prefix>    Output file prefix (without extension)
  --format <all|deb|rpm>      Package format to build (default: all)
  --libc <gnu|musl>           How the binary is linked (default: gnu). A gnu
                              .deb depends on libc6; a static musl one does not.
  -h, --help                  Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
		--version) VERSION="${2:-}"; shift 2 ;;
		--arch) ARCH_INPUT="${2:-}"; shift 2 ;;
		--binary) BINARY_PATH="${2:-}"; shift 2 ;;
		--man-dir) MAN_DIR="${2:-}"; shift 2 ;;
		--completions-dir) COMPLETIONS_DIR="${2:-}"; shift 2 ;;
		--output-prefix) OUTPUT_PREFIX="${2:-}"; shift 2 ;;
		--format) FORMAT="${2:-}"; shift 2 ;;
		--libc) LIBC="${2:-}"; shift 2 ;;
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

if [[ -z "${VERSION}" || -z "${ARCH_INPUT}" || -z "${BINARY_PATH}" || -z "${OUTPUT_PREFIX}" ]]; then
	echo "Missing required options." >&2
	usage >&2
	exit 1
fi

case "${FORMAT}" in
	all | deb | rpm) ;;
	*)
		echo "Unsupported format: ${FORMAT}" >&2
		exit 1
		;;
esac

case "${LIBC}" in
	gnu | musl) ;;
	*)
		echo "Unsupported libc: ${LIBC}" >&2
		exit 1
		;;
esac

ARCH="$(normalise_arch "${ARCH_INPUT}")"
if [[ "${ARCH}" == "universal" ]]; then
	echo "Linux packages need amd64 or arm64" >&2
	exit 1
fi

if [[ ! -f "${BINARY_PATH}" ]]; then
	echo "Built binary not found at ${BINARY_PATH}" >&2
	exit 1
fi

MAN_DIR="$(resolve_man_dir "${BINARY_PATH}" "${MAN_DIR}")"
COMPLETIONS_DIR="$(resolve_completions_dir "${BINARY_PATH}" "${COMPLETIONS_DIR}" "${MAN_DIR}")"

VERSION_NO_V="${VERSION#v}"
mkdir -p "$(dirname "${OUTPUT_PREFIX}")"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

MAN_SOURCE_DIR="${TMP_DIR}/man"
mkdir -p "${MAN_SOURCE_DIR}"
gzip -n -c "${MAN_DIR}/review-buddy.1" > "${MAN_SOURCE_DIR}/review-buddy.1.gz"

case "${ARCH}" in
	amd64)
		DEB_ARCH="amd64"
		RPM_ARCH="x86_64"
		;;
	arm64)
		DEB_ARCH="arm64"
		RPM_ARCH="aarch64"
		;;
esac

build_deb() {
	local deb_root="${TMP_DIR}/deb-root"
	mkdir -p "${deb_root}/DEBIAN" "${deb_root}/usr/bin" "${deb_root}/usr/share/man/man1"

	install -m 0755 "${BINARY_PATH}" "${deb_root}/usr/bin/review-buddy"
	install -m 0644 "${MAN_SOURCE_DIR}/review-buddy.1.gz" "${deb_root}/usr/share/man/man1/"
	stage_completions "${COMPLETIONS_DIR}" "${deb_root}/usr"
	stage_shared_files "${REPO_ROOT}" "${deb_root}/usr"

	{
		echo "Package: ${PACKAGE_NAME}"
		echo "Version: ${VERSION_NO_V}"
		echo "Section: utils"
		echo "Priority: optional"
		echo "Architecture: ${DEB_ARCH}"
		echo "Maintainer: ${PACKAGE_CONTACT}"
		echo "Homepage: ${PACKAGE_HOMEPAGE}"
		if [[ "${LIBC}" == "gnu" ]]; then
			echo "Depends: libc6"
		fi
		echo "Description: ${PACKAGE_SUMMARY}"
		echo " ${PACKAGE_DESCRIPTION}"
	} > "${deb_root}/DEBIAN/control"

	for script in postinst postrm; do
		cat > "${deb_root}/DEBIAN/${script}" <<'MAINT'
#!/bin/sh
set -e
if command -v mandb >/dev/null 2>&1; then
	mandb -q >/dev/null 2>&1 || true
fi
MAINT
		chmod 0755 "${deb_root}/DEBIAN/${script}"
	done

	local deb_output="${OUTPUT_PREFIX}.deb"
	dpkg-deb --root-owner-group --build "${deb_root}" "${deb_output}" >/dev/null
	echo "${deb_output}"
}

build_rpm() {
	if ! command -v rpmbuild >/dev/null 2>&1; then
		echo "rpmbuild is required to create RPM packages" >&2
		exit 1
	fi

	local rpm_root="${TMP_DIR}/rpm"
	mkdir -p "${rpm_root}/BUILD" "${rpm_root}/BUILDROOT" "${rpm_root}/RPMS" "${rpm_root}/SOURCES" "${rpm_root}/SPECS" "${rpm_root}/SRPMS"

	install -m 0755 "${BINARY_PATH}" "${rpm_root}/SOURCES/review-buddy"
	install -m 0644 "${MAN_SOURCE_DIR}/review-buddy.1.gz" "${rpm_root}/SOURCES/"
	stage_completions "${COMPLETIONS_DIR}" "${rpm_root}/SOURCES/stage"
	stage_shared_files "${REPO_ROOT}" "${rpm_root}/SOURCES/stage"

	cat > "${rpm_root}/SPECS/review-buddy.spec" <<SPEC
Name:           ${PACKAGE_NAME}
Version:        ${VERSION_NO_V}
Release:        1%{?dist}
Summary:        ${PACKAGE_SUMMARY}
License:        ${PACKAGE_LICENSE}
URL:            ${PACKAGE_HOMEPAGE}
Vendor:         ${PACKAGE_VENDOR}
Packager:       ${PACKAGE_CONTACT}
BuildArch:      ${RPM_ARCH}

%description
${PACKAGE_DESCRIPTION}

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/share/man/man1
install -m 0755 %{_sourcedir}/review-buddy %{buildroot}/usr/bin/review-buddy
install -m 0644 %{_sourcedir}/review-buddy.1.gz %{buildroot}/usr/share/man/man1/
cp -a %{_sourcedir}/stage/share/. %{buildroot}/usr/share/

%post
if command -v mandb >/dev/null 2>&1; then
	mandb -q >/dev/null 2>&1 || true
fi

%postun
if command -v mandb >/dev/null 2>&1; then
	mandb -q >/dev/null 2>&1 || true
fi

%files
/usr/bin/review-buddy
/usr/share/man/man1/review-buddy.1.gz
/usr/share/bash-completion/completions/review-buddy
/usr/share/zsh/site-functions/_review-buddy
/usr/share/fish/vendor_completions.d/review-buddy.fish
/usr/share/review-buddy
/usr/share/doc/review-buddy

%changelog
* $(LC_ALL=C date '+%a %b %d %Y') ${PACKAGE_CONTACT} - ${VERSION_NO_V}-1
- Package the review-buddy binary, man page, shell completions and bundled themes.
SPEC

	rpmbuild \
		--define "_topdir ${rpm_root}" \
		--define "__os_install_post %{nil}" \
		--target "${RPM_ARCH}-linux" \
		-bb "${rpm_root}/SPECS/review-buddy.spec" >/dev/null

	local rpm_built
	rpm_built="$(find "${rpm_root}/RPMS" -type f -name '*.rpm' | head -n 1)"
	if [[ -z "${rpm_built}" ]]; then
		echo "RPM build succeeded but no RPM file was produced" >&2
		exit 1
	fi

	local rpm_output="${OUTPUT_PREFIX}.rpm"
	cp "${rpm_built}" "${rpm_output}"
	echo "${rpm_output}"
}

if [[ "${FORMAT}" == "all" || "${FORMAT}" == "deb" ]]; then
	build_deb
fi

if [[ "${FORMAT}" == "all" || "${FORMAT}" == "rpm" ]]; then
	build_rpm
fi
