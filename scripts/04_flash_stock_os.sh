#!/usr/bin/env bash
# ==============================================================================
# Script: 04_flash_stock_os.sh
# Purpose: Flash raw SUPER and DTBO images via samloader-rs to restore full
#          Samsung One UI and resolve post-emergency bootloops.
# ==============================================================================

set -euo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

echo "=================================================="
echo " Samsung Stock One UI OS Flasher (SUPER + DTBO)   "
echo "=================================================="

# Check for samloader
if ! command -v samloader >/dev/null 2>&1; then
    error "samloader binary not found in PATH."
    echo "Install topjohnwu/samloader-rs before proceeding."
    exit 1
fi

IMG_DIR="${1:-.}"
SUPER_IMG="$IMG_DIR/super.img"
DTBO_IMG="$IMG_DIR/dtbo.img"

# Verify required images exist
MISSING=0
for img in "$SUPER_IMG" "$DTBO_IMG"; do
    if [[ ! -f "$img" ]] || [[ ! -s "$img" ]]; then
        error "Missing or empty image file: $img"
        MISSING=1
    fi
done

if [[ $MISSING -eq 1 ]]; then
    echo ""
    echo "Run 'scripts/02_extract_img.sh <AP_file> --full' to extract super.img and dtbo.img."
    exit 1
fi

info "Found required OS images:"
echo "  - SUPER: $SUPER_IMG ($(du -h "$SUPER_IMG" | cut -f1))"
echo "  - DTBO:  $DTBO_IMG ($(du -h "$DTBO_IMG" | cut -f1))"

# Check device connection
info "Verifying device connection in Download Mode..."
if ! samloader detect; then
    warn "Device not detected. If the phone is currently bootlooping, re-enter Download Mode:"
    echo "  1. Keep the phone plugged into your PC."
    echo "  2. Hold [Volume Down + Power] for ~8 seconds until the screen turns completely black."
    echo "  3. Immediately release and hold [Volume Up + Volume Down] together."
    echo "  4. When the teal warning screen appears, press [Volume Up] once."
    echo ""
    read -rp "Press Enter once device is in Download Mode to retry detection..." _
    if ! samloader detect; then
        error "Device still not detected by samloader. Check cable, port, and run 'sudo bash scripts/01_setup_usb.sh'."
        exit 1
    fi
fi
success "Device detected and ready."

# Confirm with user
echo ""
warn "${BOLD}ATTENTION:${NC} You are about to flash the stock SUPER (~4GB) and DTBO partitions."
warn "This will write the official One UI system image and overwrite dynamic partitions."
echo ""
if [[ "${2:-}" != "-y" && "${1:-}" != "-y" ]]; then
    read -rp "Proceed with flashing SUPER and DTBO? (y/N): " CONFIRM
    if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
        info "Flashing cancelled by user."
        exit 0
    fi
fi

# Flash SUPER and DTBO
info "Flashing raw SUPER and DTBO partitions directly via samloader-rs..."
samloader flash \
    -p SUPER "$SUPER_IMG" \
    -p DTBO "$DTBO_IMG"

echo ""
success "Flashing completed successfully!"
echo "=================================================="
echo " Post-Flash Procedure (Crucial Steps):            "
echo "=================================================="
echo "1. The device will reboot into Samsung Stock Recovery (data mismatch with One UI)."
echo "2. In Stock Recovery (use Volume keys to navigate, Power to select):"
echo "   a. Select: 'Wipe data/factory reset' -> Confirm."
echo "   b. Select: 'Reboot system now'."
echo "3. First boot into Samsung One UI setup screen takes 5 to 10 minutes."
echo "4. Once booted into One UI:"
echo "   a. Connect to Wi-Fi."
echo "   b. Go to Settings -> About phone -> Software information."
echo "   c. Tap 'Build number' 7 times to enable Developer options."
echo "   d. Open Developer options and confirm 'OEM Unlocking' is ON."
echo "5. With OEM Unlocking confirmed active, Knox/LOKE will no longer block"
echo "   custom recoveries or custom ROMs with 'FAIL! [code: -1]'."
echo "=================================================="
