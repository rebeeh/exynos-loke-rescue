#!/usr/bin/env bash
# ==============================================================================
# exynos-loke-rescue: Universal Single-Command Standard Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/rebeeh/exynos-loke-rescue/main/install.sh | bash
# ==============================================================================
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

REPO="rebeeh/exynos-loke-rescue"
BIN_NAME="exynos-loke-rescue"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
INSTALL_DIR="${INSTALL_DIR:-${DEFAULT_INSTALL_DIR}}"

echo "================================================================================"
echo "         EXYNOS LOKE RESCUE - UNIVERSAL LINUX INSTALLER"
echo "================================================================================"

# 1. Detect Architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

if [[ "${OS}" != "linux" ]]; then
    echo "ERROR: exynos-loke-rescue only supports Linux (OS detected: ${OS})"
    exit 1
fi

case "${ARCH}" in
    x86_64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    aarch64|arm64)
        TARGET="aarch64-unknown-linux-gnu"
        ;;
    armv7l)
        TARGET="armv7-unknown-linux-gnueabihf"
        ;;
    *)
        echo "ERROR: Unsupported architecture: ${ARCH}"
        exit 1
        ;;
esac

echo "==> Detected Host: Linux (${TARGET})"
mkdir -p "${INSTALL_DIR}"

# 2. Check if running inside local source repository
if [[ -f "Cargo.toml" && -d "src" && -x "$(command -v cargo 2>/dev/null)" ]]; then
    echo "==> Building from local source tree via Cargo..."
    cargo build --release
    install -m 0755 "target/release/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
else
    # 3. Download precompiled release binary or fallback to cargo
    echo "==> Fetching latest release binary from GitHub (${REPO})..."
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    RELEASE_URL="https://github.com/${REPO}/releases/latest/download/${BIN_NAME}-v1.0.0-${TARGET}.tar.gz"
    
    if curl -sSfL "${RELEASE_URL}" -o "${TMP_DIR}/release.tar.gz" 2>/dev/null; then
        echo "==> Extracting release archive..."
        tar -xzf "${TMP_DIR}/release.tar.gz" -C "${TMP_DIR}"
        EXTRACTED_BIN="$(find "${TMP_DIR}" -type f -name "${BIN_NAME}" -perm -111 | head -n 1)"
        if [[ -n "${EXTRACTED_BIN}" && -f "${EXTRACTED_BIN}" ]]; then
            install -m 0755 "${EXTRACTED_BIN}" "${INSTALL_DIR}/${BIN_NAME}"
        else
            echo "ERROR: Binary not found in release archive."
            exit 1
        fi
    elif command -v cargo >/dev/null 2>&1; then
        echo "==> Precompiled archive not yet available on release. Installing via cargo..."
        cargo install --git "https://github.com/${REPO}"
        exit 0
    else
        echo "ERROR: Unable to download release binary and cargo is not installed."
        echo "Please install cargo (rustup.rs) or download the prebuilt binary from:"
        echo "https://github.com/${REPO}/releases"
        exit 1
    fi
fi

# 4. Verify Installation
if [[ -x "${INSTALL_DIR}/${BIN_NAME}" ]]; then
    echo ""
    echo "✓ ${BIN_NAME} successfully installed to ${INSTALL_DIR}/${BIN_NAME}"
    "${INSTALL_DIR}/${BIN_NAME}" --version
else
    echo "ERROR: Installation verification failed."
    exit 1
fi

# 5. Path Check
if [[ ":${PATH}:" != *":${INSTALL_DIR}:"* ]]; then
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not currently in your PATH."
    echo "Add it to your shell configuration (~/.bashrc or ~/.zshrc):"
    echo "    export PATH=\"\${HOME}/.local/bin:\${PATH}\""
fi

# 6. USB Permissions Instruction
echo ""
echo "==> USB Device Permissions Setup:"
echo "To grant your Linux user permission to flash Samsung Download Mode devices (04e8:685d),"
echo "run the built-in USB permission configuration command:"
echo ""
echo "    sudo ${INSTALL_DIR}/${BIN_NAME} fix-usb"
echo ""
echo "================================================================================"
