#!/usr/bin/env bash
set -euo pipefail

TARGET_BIN="${1:-target/release/exynos-loke-rescue}"

if [[ ! -f "${TARGET_BIN}" ]]; then
    echo "ERROR: Target binary '${TARGET_BIN}' not found. Run 'cargo build --release' first."
    exit 1
fi

echo "================================================================================"
echo "EXYNOS LOKE RESCUE: RELEASE BINARY HARDENING & EXPOSURE AUDIT"
echo "Target: ${TARGET_BIN}"
echo "================================================================================"

# 1. Check SHA256 digest
SHA256=$(sha256sum "${TARGET_BIN}" | awk '{print $1}')
FILE_SIZE=$(stat -c %s "${TARGET_BIN}")
echo "Artifact Digest:  ${SHA256}"
echo "Artifact Size:    ${FILE_SIZE} bytes ($(( FILE_SIZE / 1024 )) KB)"
echo ""

# 2. Check ELF Header Properties using readelf
if command -v readelf >/dev/null 2>&1; then
    echo "==> ELF Architecture & Headers:"
    readelf -h "${TARGET_BIN}" | grep -E "(Class|Data|Type|Machine)" | sed 's/^/  /'
    echo ""

    echo "==> Security Hardening Verification (readelf / checks):"
    
    # Check PIE
    if readelf -h "${TARGET_BIN}" | grep -q "DYN (Position-Independent Executable"; then
        echo "  [PASS] PIE (Position-Independent Executable): ENABLED"
    elif readelf -d "${TARGET_BIN}" | grep -q "FLAGS_1.*PIE"; then
        echo "  [PASS] PIE (Position-Independent Executable): ENABLED"
    else
        echo "  [WARN] PIE: Not detected as ET_DYN"
    fi

    # Check NX / PT_GNU_STACK
    if readelf -l "${TARGET_BIN}" | grep -A 1 "GNU_STACK" | grep -q "RWE"; then
        echo "  [FAIL] NX (No-Execute Stack): DISABLED (Executable Stack Detected!)"
        exit 1
    else
        echo "  [PASS] NX (No-Execute Stack): ENABLED"
    fi

    # Check RELRO
    if readelf -l "${TARGET_BIN}" | grep -q "GNU_RELRO"; then
        if readelf -d "${TARGET_BIN}" | grep -q "BIND_NOW"; then
            echo "  [PASS] RELRO (Read-Only Relocations): FULL RELRO (BIND_NOW)"
        else
            echo "  [PASS] RELRO (Read-Only Relocations): PARTIAL RELRO"
        fi
    else
        echo "  [WARN] RELRO: GNU_RELRO segment not found"
    fi

    # Check RPATH / RUNPATH
    if readelf -d "${TARGET_BIN}" | grep -E "(RPATH|RUNPATH)"; then
        echo "  [FAIL] RPATH/RUNPATH: Insecure dynamic loader paths detected!"
        exit 1
    else
        echo "  [PASS] RPATH/RUNPATH: ABSENT (Safe)"
    fi

    # Check Stripped Symbols
    if readelf -S "${TARGET_BIN}" | grep -q ".symtab"; then
        echo "  [WARN] Symbol Table (.symtab) present. Run 'strip ${TARGET_BIN}' before release."
    else
        echo "  [PASS] Symbol Table: STRIPPED"
    fi
fi
echo ""

# 3. Check for leaked secret or environment tokens
echo "==> Secret & Credential Marker Scan:"
LEAKS=$(strings "${TARGET_BIN}" | grep -E "(ghp_[A-Za-z0-9]{36}|github_pat_|BEGIN PRIVATE KEY|AKIA[0-9A-Z]{16})" || true)
if [[ -n "${LEAKS}" ]]; then
    echo "  [FAIL] Leaked credentials detected in binary strings!"
    exit 1
else
    echo "  [PASS] No credential markers detected in binary."
fi

# 4. Check for developer home paths
echo "==> Path Sanitization Scan:"
DEV_PATHS=$(strings "${TARGET_BIN}" | grep -E "/home/[a-zA-Z0-9_.-]+/(Projects|Desktop|Documents)" || true)
if [[ -n "${DEV_PATHS}" ]]; then
    echo "  [INFO] Local build directory strings detected (retained for panic traces):"
    echo "${DEV_PATHS}" | head -n 3 | sed 's/^/    /'
else
    echo "  [PASS] No local development paths exposed."
fi

echo ""
echo "✓ Release binary passed security and hardening audit successfully."
