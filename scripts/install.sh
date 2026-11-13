#!/usr/bin/env bash
set -euo pipefail

echo "==> Installing system dependencies (Fedora / RHEL / Debian / Ubuntu)..."
if command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y rust cargo curl unzip usbutils lz4
elif command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update && sudo apt-get install -y cargo rustc curl unzip usbutils liblz4-tool
fi

echo "==> Fetching latest topjohnwu/samloader-rs Linux x86_64 release..."
LATEST_URL=$(curl -fsSL https://api.github.com/repos/topjohnwu/samloader-rs/releases/latest \
    | grep -oE '"browser_download_url":\s*"[^"]+linux[^"]+x86_64[^"]+"' \
    | cut -d '"' -f 4 | head -n 1)

if [[ -n "${LATEST_URL}" ]]; then
    TMP_DIR=$(mktemp -d)
    curl -fL "${LATEST_URL}" -o "${TMP_DIR}/samloader.tar.gz"
    tar -xzf "${TMP_DIR}/samloader.tar.gz" -C "${TMP_DIR}"
    chmod +x "${TMP_DIR}/samloader"
    sudo mv "${TMP_DIR}/samloader" /usr/local/bin/samloader
    rm -rf "${TMP_DIR}"
    echo "✓ Installed /usr/local/bin/samloader"
else
    echo "⚠ Could not resolve samloader-rs release URL automatically; install manually."
fi

echo "==> Building exynos-loke-rescue in release mode..."
cargo build --release
sudo cp target/release/exynos-loke-rescue /usr/local/bin/exynos-loke-rescue

echo "==> Installing udev rules..."
sudo cp udev/60-exynos-loke-rescue.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "✓ Installation complete. Run: exynos-loke-rescue prep-usb"
