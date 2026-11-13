#!/usr/bin/env bash
# ==============================================================================
# Script: 02_extract_img.sh
# Purpose: Extract signed boot, recovery, and vbmeta lz4 images from stock AP
#          tarball and decompress them to raw .img files for targeted flashing.
# ==============================================================================

set -euo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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
echo " Samsung Stock AP Boot Component Extractor       "
echo "=================================================="

# Check for required tools
for tool in tar lz4; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        error "Required tool '$tool' is not installed."
        echo "Install it using your package manager:"
        echo "  Fedora:  sudo dnf install -y $tool"
        echo "  Ubuntu:  sudo apt install -y $tool"
        echo "  Arch:    sudo pacman -S $tool"
        exit 1
    fi
done

AP_FILE="${1:-}"

# Auto-detect AP tarball if not supplied
if [[ -z "$AP_FILE" ]]; then
    SEARCH_PATHS=(
        "."
        "./stock_fw"
        "$HOME/stock_fw"
        "./firmware"
    )

    for p in "${SEARCH_PATHS[@]}"; do
        MATCHES=$(find "$p" -maxdepth 2 \( -name "AP_*.tar.md5" -o -name "AP_*.tar" \) 2>/dev/null || true)
        if [[ -n "$MATCHES" ]]; then
            AP_FILE=$(echo "$MATCHES" | head -n 1)
            info "Auto-detected stock AP archive: $AP_FILE"
            break
        fi
    done
fi

if [[ -z "$AP_FILE" ]] || [[ ! -f "$AP_FILE" ]]; then
    error "Could not find a valid AP tarball (AP_*.tar.md5 or AP_*.tar)."
    echo ""
    echo "Usage:"
    echo "  $0 [path/to/AP_*.tar.md5]"
    echo ""
    echo "Example:"
    echo "  $0 ~/stock_fw/AP_A217FXXUBCVG1_...tar.md5"
    exit 1
fi

info "Using AP archive: $AP_FILE"

REQUIRED_LZ4_FILES=("boot.img.lz4" "recovery.img.lz4" "vbmeta.img.lz4")
OUTPUT_IMAGES=("boot.img" "recovery.img" "vbmeta.img")

# Step 1: Extract lz4 components from the tar archive
info "Extracting boot components from AP archive..."
tar -xf "$AP_FILE" "${REQUIRED_LZ4_FILES[@]}"

for f in "${REQUIRED_LZ4_FILES[@]}"; do
    if [[ ! -f "$f" ]]; then
        error "Failed to extract expected component: $f from $AP_FILE"
        exit 1
    fi
    success "Extracted: $f ($(du -h "$f" | cut -f1))"
done

# Step 2: Decompress lz4 images
info "Decompressing lz4 archives to raw flashable .img files..."
for f in "${REQUIRED_LZ4_FILES[@]}"; do
    target_img="${f%.lz4}"
    info "Decompressing $f -> $target_img..."
    lz4 -df "$f" "$target_img"
    if [[ ! -s "$target_img" ]]; then
        error "Decompression failed or output is empty for: $target_img"
        exit 1
    fi
    success "Decompressed: $target_img ($(du -h "$target_img" | cut -f1))"
done

echo ""
echo "=================================================="
echo " Extracted Signed Images Ready for Flashing       "
echo "=================================================="
for img in "${OUTPUT_IMAGES[@]}"; do
    ls -lh "$img"
done
echo "=================================================="
echo ""
success "Extraction and decompression complete!"
info "Next step: Run 'scripts/03_flash_signed.sh' to flash components via samloader-rs."
