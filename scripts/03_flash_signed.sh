#!/usr/bin/env bash
# ==============================================================================
# Script: 03_flash_signed.sh
# Purpose: Flash signed stock BOOT, VBMETA, and RECOVERY images using samloader-rs
#          to bypass LOKE session timeouts and dynamic partition metadata crashes.
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
echo " Samsung Exynos Signed Boot Component Flasher     "
echo "=================================================="

# Check for samloader
if ! command -v samloader >/dev/null 2>&1; then
    error "samloader binary not found in PATH."
    echo "Install topjohnwu/samloader-rs (https://github.com/topjohnwu/samloader-rs):"
    echo "  curl -s https://api.github.com/repos/topjohnwu/samloader-rs/releases/latest \\"
    echo "    | grep \"browser_download_url.*linux.*x86_64\" \\"
    echo "    | cut -d '\"' -f 4 \\"
    echo "    | xargs curl -L -o samloader-rs.tar.gz"
    echo "  tar -xzf samloader-rs.tar.gz && chmod +x samloader"
    echo "  sudo mv samloader /usr/local/bin/"
    exit 1
fi

IMG_DIR="${1:-.}"
BOOT_IMG="$IMG_DIR/boot.img"
RECOVERY_IMG="$IMG_DIR/recovery.img"
VBMETA_IMG="$IMG_DIR/vbmeta.img"

# Verify all required images exist
MISSING=0
for img in "$BOOT_IMG" "$RECOVERY_IMG" "$VBMETA_IMG"; do
    if [[ ! -f "$img" ]] || [[ ! -s "$img" ]]; then
        error "Missing or empty image file: $img"
        MISSING=1
    fi
done

if [[ $MISSING -eq 1 ]]; then
    echo ""
    echo "Run 'scripts/02_extract_img.sh' first to extract these files from your AP tarball."
    exit 1
fi

info "Found required signed images:"
echo "  - BOOT:     $BOOT_IMG ($(du -h "$BOOT_IMG" | cut -f1))"
echo "  - VBMETA:   $VBMETA_IMG ($(du -h "$VBMETA_IMG" | cut -f1))"
echo "  - RECOVERY: $RECOVERY_IMG ($(du -h "$RECOVERY_IMG" | cut -f1))"

# Check device connection
info "Verifying device connection in Download / Emergency Recovery Mode..."
if ! samloader detect; then
    error "Could not detect device with samloader."
    echo "Troubleshooting steps:"
    echo "  1. Run: sudo bash scripts/01_setup_usb.sh"
    echo "  2. Unplug and re-insert the USB cable."
    echo "  3. Ensure device screen displays Download Mode or 'An error has occurred...'"
    exit 1
fi
success "Device connected and responding."

# Confirm with user
echo ""
warn "${BOLD}ATTENTION:${NC} You are about to flash signed stock boot components."
warn "This will overwrite existing boot, recovery, and vbmeta partitions to reset the LOKE session flag."
echo ""
if [[ "${2:-}" != "-y" && "${1:-}" != "-y" ]]; then
    read -rp "Proceed with flashing? (y/N): " CONFIRM
    if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
        info "Flashing cancelled by user."
        exit 0
    fi
fi

# Flash signed images
info "Flashing signed stock components via samloader-rs..."
samloader flash \
    -p BOOT "$BOOT_IMG" \
    -p VBMETA "$VBMETA_IMG" \
    -p RECOVERY "$RECOVERY_IMG"

echo ""
success "Flashing completed!"
echo "=================================================="
echo " Post-Flash Procedure (Crucial Steps):            "
echo "=================================================="
echo "1. The device will reboot automatically."
echo "2. Because the dynamic 'super' partition is still in custom ROM layout,"
echo "   the phone will automatically enter Samsung Stock Recovery."
echo "3. In Stock Recovery (use Volume keys to navigate, Power to select):"
echo "   a. Select: 'Wipe data/factory reset' -> Confirm."
echo "   b. Select: 'Reboot to bootloader' to enter a clean Download Mode."
echo "4. Your device is now unbricked!"
echo "   You can now safely flash your preferred custom recovery (e.g. Lineage Recovery)"
echo "   and proceed with your custom ROM sideload."
echo "=================================================="
