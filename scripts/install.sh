#!/usr/bin/env bash
set -euo pipefail

if command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y rust cargo curl unzip usbutils lz4
elif command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update && sudo apt-get install -y cargo rustc curl unzip usbutils liblz4-tool
fi

cargo build --release
sudo cp target/release/exynos-loke-rescue /usr/local/bin/exynos-loke-rescue
sudo cp udev/60-exynos-loke-rescue.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "✓ Installed /usr/local/bin/exynos-loke-rescue"
