#!/usr/bin/env bash
# ==============================================================================
# Script: 01_setup_usb.sh
# Purpose: Configure Linux USB permissions, stop ModemManager interference,
#          unload conflicting kernel modules, and detect Samsung Download Mode.
# ==============================================================================

set -euo pipefail

# ANSI color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

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
echo " Samsung Exynos USB Environment Setup (Linux)    "
echo "=================================================="

# Check for root / sudo access
if [[ $EUID -ne 0 ]]; then
    warn "This script requires superuser privileges to configure udev and services."
    echo "Re-running with sudo..."
    exec sudo bash "$0" "$@"
fi

# Step 1: Check and apply samloader udev rules
info "Configuring USB udev rules for Samsung Download Mode..."
if command -v samloader >/dev/null 2>&1; then
    samloader fix-usb || warn "samloader fix-usb encountered a non-fatal warning."
    success "samloader udev rules configured."
else
    warn "samloader command not found in PATH."
    info "Applying fallback udev rule for Samsung USB Vendor ID 04e8..."
    UDEV_RULE_FILE="/etc/udev/rules.d/99-samsung-download-mode.rules"
    cat << 'EOF' > "$UDEV_RULE_FILE"
# Samsung Download Mode / LOKE Protocol USB rule
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", MODE="0666", GROUP="plugdev"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="685d", MODE="0666", GROUP="plugdev"
SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6860", MODE="0666", GROUP="plugdev"
EOF
    udevadm control --reload-rules || true
    udevadm trigger || true
    success "Fallback udev rules written to $UDEV_RULE_FILE and reloaded."
fi

# Step 2: Stop and disable ModemManager interference
info "Stopping ModemManager to prevent USB serial port locking..."
if systemctl list-unit-files | grep -q "^ModemManager.service"; then
    systemctl stop ModemManager || warn "Could not stop ModemManager (it may not be active)."
    success "ModemManager stopped."
else
    info "ModemManager service not found; skipping."
fi

# Step 3: Unload conflicting cdc_acm module
info "Unloading conflicting cdc_acm kernel module..."
if lsmod | grep -q "^cdc_acm"; then
    modprobe -r cdc_acm || warn "Could not unload cdc_acm module (might be in use)."
    success "cdc_acm module unloaded."
else
    info "cdc_acm is not loaded; skipping."
fi

# Step 4: Verify Samsung USB Device in Download Mode
info "Scanning USB bus for Samsung devices (Vendor ID: 04e8)..."
if command -v lsusb >/dev/null 2>&1; then
    SAMSUNG_DEV=$(lsusb -d 04e8: || true)
    if [[ -n "$SAMSUNG_DEV" ]]; then
        success "Detected Samsung USB device:"
        echo "  $SAMSUNG_DEV"
    else
        warn "No Samsung device found matching Vendor ID 04e8 via lsusb."
        echo "  Make sure your device is connected via USB and in Download / Emergency Recovery Mode."
    fi
else
    warn "lsusb not installed. Skipping raw USB enumeration check."
fi

# Step 5: Test device detection with samloader
if command -v samloader >/dev/null 2>&1; then
    info "Running 'samloader detect'..."
    if samloader detect; then
        success "Device successfully detected by samloader!"
    else
        warn "samloader detect did not detect the device. Please reconnect the USB cable and verify Download Mode."
    fi
else
    warn "samloader binary not found in PATH (/usr/local/bin or ~/.cargo/bin)."
    echo "  Install topjohnwu/samloader-rs before proceeding to flash."
fi

echo ""
success "USB setup complete. Next step: Run 'scripts/02_extract_img.sh' with your stock AP tarball."
