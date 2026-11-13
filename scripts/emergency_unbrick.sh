#!/usr/bin/env bash
set -euo pipefail

STOCK_DIR="${1:-$HOME/Downloads/stock}"

echo "============================================================"
echo "  EXYNOS LOKE RESCUE - ONE-SHOT EMERGENCY RECOVERY PIPELINE"
echo "============================================================"

exynos-loke-rescue prep-usb

if ! compgen -G "${STOCK_DIR}/AP_*.tar.md5" >/dev/null; then
    echo "==> No AP archive found in ${STOCK_DIR}. Auto-fetching matching firmware..."
    exynos-loke-rescue fetch-stock --out-dir "${STOCK_DIR}"
fi

echo "==> Executing signed boot recovery (BOOT + VBMETA + RECOVERY)..."
exynos-loke-rescue rescue-boot --ap "${STOCK_DIR}"

echo "✓ Done. If the device bootloops due to leftover custom ROM partitions,"
echo "  re-enter Download Mode and run:"
echo "  exynos-loke-rescue full-raw-restore --stock-dir ${STOCK_DIR}"
