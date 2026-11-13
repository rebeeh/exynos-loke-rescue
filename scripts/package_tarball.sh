#!/usr/bin/env bash
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

VERSION="${1:-1.0.0}"
TARGET="${2:-$(rustc -vV | sed -n 's|host: ||p')}"
DIST_DIR="dist"
ARCHIVE_NAME="exynos-loke-rescue-v${VERSION}-${TARGET}"
STAGE_DIR="${DIST_DIR}/${ARCHIVE_NAME}"

echo "==> Assembling release distribution: ${ARCHIVE_NAME}"
rm -rf "${STAGE_DIR}" "${DIST_DIR}/${ARCHIVE_NAME}.tar.gz"
mkdir -p "${STAGE_DIR}/udev" "${STAGE_DIR}/man" "${STAGE_DIR}/completions"

# Copy binary
cp "target/release/exynos-loke-rescue" "${STAGE_DIR}/exynos-loke-rescue"
chmod 755 "${STAGE_DIR}/exynos-loke-rescue"

# Copy udev rules
cp "udev/60-exynos-loke-rescue.rules" "${STAGE_DIR}/udev/"

# Copy man pages and completions
if [[ -d "target/man" ]]; then
    cp -r target/man/* "${STAGE_DIR}/man/"
fi
if [[ -d "target/completions" ]]; then
    cp -r target/completions/* "${STAGE_DIR}/completions/"
fi

# Copy docs and licenses
cp README.md PROTOCOL.md LICENSE "${STAGE_DIR}/"

# Write standalone installer script inside the tarball
cat << 'EOF' > "${STAGE_DIR}/install.sh"
#!/usr/bin/env bash
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${PREFIX}/bin"
RULES_DIR="/etc/udev/rules.d"
MAN_DIR="${PREFIX}/share/man/man1"
BASH_COMP="/usr/share/bash-completion/completions"
ZSH_COMP="/usr/share/zsh/site-functions"
FISH_COMP="/usr/share/fish/vendor_completions.d"

echo "==> Installing exynos-loke-rescue to ${BIN_DIR}..."
sudo install -D -m 0755 exynos-loke-rescue "${BIN_DIR}/exynos-loke-rescue"

echo "==> Installing udev rules to ${RULES_DIR}..."
sudo install -D -m 0644 udev/60-exynos-loke-rescue.rules "${RULES_DIR}/60-exynos-loke-rescue.rules"
if command -v udevadm >/dev/null 2>&1; then
    sudo udevadm control --reload-rules || true
    sudo udevadm trigger --subsystem-match=usb || true
fi

if [[ -f "man/exynos-loke-rescue.1" ]]; then
    echo "==> Installing man page..."
    sudo install -D -m 0644 man/exynos-loke-rescue.1 "${MAN_DIR}/exynos-loke-rescue.1"
fi

if [[ -d "${BASH_COMP}" && -f "completions/exynos-loke-rescue.bash" ]]; then
    sudo install -D -m 0644 completions/exynos-loke-rescue.bash "${BASH_COMP}/exynos-loke-rescue"
fi
if [[ -d "${ZSH_COMP}" && -f "completions/_exynos-loke-rescue" ]]; then
    sudo install -D -m 0644 completions/_exynos-loke-rescue "${ZSH_COMP}/_exynos-loke-rescue"
fi
if [[ -d "${FISH_COMP}" && -f "completions/exynos-loke-rescue.fish" ]]; then
    sudo install -D -m 0644 completions/exynos-loke-rescue.fish "${FISH_COMP}/exynos-loke-rescue.fish"
fi

echo "✓ Successfully installed exynos-loke-rescue."
EOF
chmod 755 "${STAGE_DIR}/install.sh"

# Create compressed tarball and checksum
tar -czf "${DIST_DIR}/${ARCHIVE_NAME}.tar.gz" -C "${DIST_DIR}" "${ARCHIVE_NAME}"
cd "${DIST_DIR}"
sha256sum "${ARCHIVE_NAME}.tar.gz" > "${ARCHIVE_NAME}.tar.gz.sha256"
cd ..

echo "✓ Created: ${DIST_DIR}/${ARCHIVE_NAME}.tar.gz"
echo "  SHA256: $(cat "${DIST_DIR}/${ARCHIVE_NAME}.tar.gz.sha256" | awk '{print $1}')"
