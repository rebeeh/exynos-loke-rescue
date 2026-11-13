#!/usr/bin/env bash
set -euo pipefail

BL_ARCHIVE="${1:?Usage: $0 <path/to/BL_*.tar.md5>}"
OUT_DIR="${2:-./rea_sboot_workspace}"

mkdir -p "${OUT_DIR}"
tar -xf "${BL_ARCHIVE}" -C "${OUT_DIR}" sboot.bin.lz4
lz4 -df "${OUT_DIR}/sboot.bin.lz4" "${OUT_DIR}/sboot.bin"

echo "[OK] Decompressed sboot.bin ready for REA / Ghidra analysis at:"
echo "  ${OUT_DIR}/sboot.bin (Load as AArch64 Little Endian, Base: 0x8F000000)"
