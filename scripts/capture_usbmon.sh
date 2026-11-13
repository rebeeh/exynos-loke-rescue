#!/usr/bin/env bash
set -euo pipefail

OUT_PCAP="${1:-$HOME/loke_wire_capture.pcapng}"
sudo modprobe usbmon
sudo setfacl -m "u:${USER}:r" /dev/usbmon*

BUS=$(lsusb | grep -i "04e8:685d" | awk '{print $2}' | sed 's/^0*//' || true)
if [[ -z "${BUS}" ]]; then
    echo "ERROR: Samsung Download Mode (04e8:685d) not found on USB bus."
    exit 1
fi

echo "==> Capturing USB bulk frames on usbmon${BUS} -> ${OUT_PCAP}"
echo "    Run your flasher in another terminal, then press Ctrl+C here."
tshark -i "usbmon${BUS}" -w "${OUT_PCAP}" -Y "usb.transfer_type == 0x03"
