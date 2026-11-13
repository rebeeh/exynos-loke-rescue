#!/usr/bin/env bash
# ==============================================================================
# Script: 02_extract_img.sh
# Purpose: Extract signed boot components (boot, recovery, vbmeta) or full OS
#          images (super, dtbo) from stock AP tarball and decompress with lz4.
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
echo " Samsung Stock AP Image Extractor & Decompressor  "
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

AP_FILE=""
MODE="boot" # default: boot components only

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --full|--all|--super|--os)
            MODE="full"
            ;;
        --help|-h)
            echo "Usage: $0 [path/to/AP_*.tar.md5] [--full]"
            echo ""
            echo "Options:"
            echo "  --full    Extract both boot components AND full OS partitions (super.img & dtbo.img)"
            echo "            Use this to restore full stock One UI to resolve post-rescue bootloop."
            exit 0
            ;;
        *)
            if [[ -f "$arg" ]]; then
                AP_FILE="$arg"
            fi
            ;;
    esac
done

# Auto-detect AP tarball if not supplied
if [[ -z "$AP_FILE" ]]; then
    SEARCH_PATHS=(
        "."
        "./stock_fw"
        "$HOME/Downloads/stock"
        "$HOME/Downloads"
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
    echo "  $0 [path/to/AP_*.tar.md5] [--full]"
    echo ""
    echo "Examples:"
    echo "  $0 stock_fw/AP_*.tar.md5           # Extract boot, recovery, vbmeta (Fast, unbrick)"
    echo "  $0 stock_fw/AP_*.tar.md5 --full    # Also extract super.img and dtbo.img (Full stock OS)"
    exit 1
fi

info "Using AP archive: $AP_FILE"

if [[ "$MODE" == "full" ]]; then
    info "Mode: FULL OS EXTRACTION (boot, recovery, vbmeta, dtbo, super)"
    warn "Note: super.img is over 4 GB; extraction and decompression will take 1-3 minutes."
    REQUIRED_LZ4_FILES=("boot.img.lz4" "recovery.img.lz4" "vbmeta.img.lz4" "dtbo.img.lz4" "super.img.lz4")
else
    info "Mode: BOOT COMPONENTS ONLY (boot, recovery, vbmeta)"
    info "Tip: To also extract super.img and dtbo.img for full stock One UI, pass '--full'."
    REQUIRED_LZ4_FILES=("boot.img.lz4" "recovery.img.lz4" "vbmeta.img.lz4")
fi

# Step 1: Extract lz4 components from the tar archive
info "Extracting requested components from AP archive..."
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
echo " Extracted Images Ready for Flashing              "
echo "=================================================="
for f in "${REQUIRED_LZ4_FILES[@]}"; do
    img="${f%.lz4}"
    ls -lh "$img"
done
echo "=================================================="
echo ""
success "Extraction and decompression complete!"

if [[ "$MODE" == "full" ]]; then
    info "Next step: Run 'scripts/04_flash_stock_os.sh' to flash SUPER and DTBO partitions."
else
    info "Next step: Run 'scripts/03_flash_signed.sh' to clear the LOKE emergency recovery lock."
fi
