#!/usr/bin/env bash
# Standalone unprivileged Debian (.deb) package builder
set -euo pipefail

VERSION="${1:-1.0.0}"
ARCH="${2:-amd64}"
PKG_NAME="exynos-loke-rescue"
DIST_DIR="dist"
DEB_DIR="${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}"

echo "==> Building Debian package: ${PKG_NAME}_${VERSION}_${ARCH}.deb"
rm -rf "${DEB_DIR}"
mkdir -p "${DEB_DIR}/DEBIAN" \
         "${DEB_DIR}/usr/bin" \
         "${DEB_DIR}/usr/lib/udev/rules.d" \
         "${DEB_DIR}/usr/share/man/man1" \
         "${DEB_DIR}/usr/share/doc/${PKG_NAME}" \
         "${DEB_DIR}/usr/share/bash-completion/completions" \
         "${DEB_DIR}/usr/share/zsh/vendor-completions" \
         "${DEB_DIR}/usr/share/fish/vendor_completions.d"

# Copy binary
cp "target/release/${PKG_NAME}" "${DEB_DIR}/usr/bin/${PKG_NAME}"
chmod 755 "${DEB_DIR}/usr/bin/${PKG_NAME}"

# Copy udev rules
cp "udev/60-exynos-loke-rescue.rules" "${DEB_DIR}/usr/lib/udev/rules.d/"
chmod 644 "${DEB_DIR}/usr/lib/udev/rules.d/60-exynos-loke-rescue.rules"

# Copy documentation
cp README.md PROTOCOL.md "${DEB_DIR}/usr/share/doc/${PKG_NAME}/"
cp LICENSE "${DEB_DIR}/usr/share/doc/${PKG_NAME}/copyright"

# Copy man pages
if [[ -f "target/man/${PKG_NAME}.1" ]]; then
    gzip -c -9 "target/man/${PKG_NAME}.1" > "${DEB_DIR}/usr/share/man/man1/${PKG_NAME}.1.gz"
fi

# Copy shell completions
if [[ -f "target/completions/${PKG_NAME}.bash" ]]; then
    cp "target/completions/${PKG_NAME}.bash" "${DEB_DIR}/usr/share/bash-completion/completions/${PKG_NAME}"
fi
if [[ -f "target/completions/_${PKG_NAME}" ]]; then
    cp "target/completions/_${PKG_NAME}" "${DEB_DIR}/usr/share/zsh/vendor-completions/_${PKG_NAME}"
fi
if [[ -f "target/completions/${PKG_NAME}.fish" ]]; then
    cp "target/completions/${PKG_NAME}.fish" "${DEB_DIR}/usr/share/fish/vendor_completions.d/${PKG_NAME}.fish"
fi

# Write control file
cat << EOF > "${DEB_DIR}/DEBIAN/control"
Package: ${PKG_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: Rabeeh Fouz Ahammad VK <rabeeh@users.noreply.github.com>
Description: Deterministic Linux firmware recovery engine for Samsung Exynos SoCs
 Exynos LOKE Rescue is a clean-room, memory-safe Linux-native firmware flashing
 engine and deterministic partition recovery tool for Samsung Exynos SoCs.
 Interfaces directly with Odin/LOKE USB endpoints (04e8:685d).
EOF

# Copy maintainer scripts
cp "packaging/debian/postinst" "${DEB_DIR}/DEBIAN/postinst"
cp "packaging/debian/prerm" "${DEB_DIR}/DEBIAN/prerm"
chmod 755 "${DEB_DIR}/DEBIAN/postinst" "${DEB_DIR}/DEBIAN/prerm"

# Build package with dpkg-deb if available, otherwise assemble tar archive
if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --build --root-owner-group "${DEB_DIR}" "${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
    echo "✓ Built Debian package: ${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
else
    # Fallback to standard ar archive
    TMP_ARCH=$(mktemp -d)
    echo "2.0" > "${TMP_ARCH}/debian-binary"
    tar -czf "${TMP_ARCH}/control.tar.gz" -C "${DEB_DIR}/DEBIAN" .
    tar -czf "${TMP_ARCH}/data.tar.gz" -C "${DEB_DIR}" --exclude='./DEBIAN' .
    ar rcs "${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb" \
        "${TMP_ARCH}/debian-binary" \
        "${TMP_ARCH}/control.tar.gz" \
        "${TMP_ARCH}/data.tar.gz"
    rm -rf "${TMP_ARCH}"
    echo "✓ Built Debian package via ar: ${DIST_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
fi
