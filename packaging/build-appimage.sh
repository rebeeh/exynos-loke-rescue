#!/usr/bin/env bash
# Standalone AppImage bundler for exynos-loke-rescue
set -euo pipefail

PKG_NAME="exynos-loke-rescue"
VERSION="${1:-1.0.0}"
DIST_DIR="dist"
APPDIR="${DIST_DIR}/${PKG_NAME}.AppDir"

echo "==> Preparing AppDir bundle for ${PKG_NAME}..."
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin" "${APPDIR}/usr/share/metainfo" "${APPDIR}/usr/lib/udev/rules.d"

# Copy binary & udev rules
cp "target/release/${PKG_NAME}" "${APPDIR}/usr/bin/"
cp "udev/60-exynos-loke-rescue.rules" "${APPDIR}/usr/lib/udev/rules.d/"

# Create desktop file
cat << EOF > "${APPDIR}/${PKG_NAME}.desktop"
[Desktop Entry]
Name=Exynos LOKE Rescue
Exec=exynos-loke-rescue
Icon=exynos-loke-rescue
Type=Application
Categories=Utility;System;
Comment=Linux firmware recovery engine for Samsung Exynos SoCs
Terminal=true
EOF

# Create AppRun entrypoint
cat << 'EOF' > "${APPDIR}/AppRun"
#!/usr/bin/env bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/exynos-loke-rescue" "$@"
EOF
chmod 755 "${APPDIR}/AppRun"

# Dummy icon
touch "${APPDIR}/${PKG_NAME}.png"

echo "✓ AppDir prepared at ${APPDIR}. In CI, appimagetool packages this into ${DIST_DIR}/${PKG_NAME}-${VERSION}-x86_64.AppImage"
