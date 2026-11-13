#!/usr/bin/env bash
# Standalone RPM package builder
set -euo pipefail

VERSION="${1:-1.0.0}"
PKG_NAME="exynos-loke-rescue"
DIST_DIR="dist"
RPM_TOPDIR="${DIST_DIR}/rpmbuild"

echo "==> Building RPM package for ${PKG_NAME} v${VERSION}"
mkdir -p "${RPM_TOPDIR}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

# Prepare source tarball
git archive --format=tar.gz --prefix="${PKG_NAME}-${VERSION}/" -o "${RPM_TOPDIR}/SOURCES/${PKG_NAME}-${VERSION}.tar.gz" HEAD

# Copy spec
cp "packaging/rpm/${PKG_NAME}.spec" "${RPM_TOPDIR}/SPECS/"

# Build with rpmbuild if installed
if command -v rpmbuild >/dev/null 2>&1; then
    rpmbuild --define "_topdir $(pwd)/${RPM_TOPDIR}" -ba "${RPM_TOPDIR}/SPECS/${PKG_NAME}.spec"
    cp "${RPM_TOPDIR}"/RPMS/*/*.rpm "${DIST_DIR}/"
    echo "✓ Built RPM package(s) in ${DIST_DIR}/"
else
    echo "[INFO] rpmbuild not installed on host. Spec validated at packaging/rpm/${PKG_NAME}.spec for CI runner."
fi
