#!/usr/bin/env bash
# ==============================================================================
# exynos-loke-rescue: Release Artifact Packager
# Standard standalone binary distribution matching samloader-rs release format
# ==============================================================================
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

VERSION="${1:-1.0.0}"
ARCH="${2:-x86_64}"
TARGET="${3:-}"

if [[ -z "${TARGET}" ]]; then
    case "${ARCH}" in
        x86_64)
            TARGET="x86_64-unknown-linux-musl"
            ;;
        aarch64|arm64)
            ARCH="aarch64"
            TARGET="aarch64-unknown-linux-musl"
            ;;
        *)
            TARGET="x86_64-unknown-linux-musl"
            ;;
    esac
fi

DIST_DIR="dist"
mkdir -p "${DIST_DIR}"

# Locate binary
BIN_PATH=""
if [[ -f "target/${TARGET}/release/exynos-loke-rescue" ]]; then
    BIN_PATH="target/${TARGET}/release/exynos-loke-rescue"
elif [[ -f "target/release/exynos-loke-rescue" ]]; then
    BIN_PATH="target/release/exynos-loke-rescue"
else
    echo "ERROR: Release binary not found for target ${TARGET}"
    exit 1
fi

echo "==> Packaging standalone release for linux-${ARCH} from ${BIN_PATH}..."
chmod 755 "${BIN_PATH}"

ZIP_NAME="exynos-loke-rescue-v${VERSION}-linux-${ARCH}.zip"
TAR_NAME="exynos-loke-rescue-v${VERSION}-linux-${ARCH}.tar.gz"

# 1. Standalone zip archive (identical to samloader-rs release format)
TMP_STAGE="$(mktemp -d)"
trap 'rm -rf "${TMP_STAGE}"' EXIT

cp "${BIN_PATH}" "${TMP_STAGE}/exynos-loke-rescue"

cd "${TMP_STAGE}"
if command -v zip >/dev/null 2>&1; then
    zip -9 -j "${ZIP_NAME}" exynos-loke-rescue
else
    python3 -c "import zipfile; zf = zipfile.ZipFile('${ZIP_NAME}', 'w', zipfile.ZIP_DEFLATED); zf.write('exynos-loke-rescue', 'exynos-loke-rescue'); zf.close()"
fi
tar -czf "${TAR_NAME}" exynos-loke-rescue
cd - >/dev/null

mv "${TMP_STAGE}/${ZIP_NAME}" "${DIST_DIR}/${ZIP_NAME}"
mv "${TMP_STAGE}/${TAR_NAME}" "${DIST_DIR}/${TAR_NAME}"

cd "${DIST_DIR}"
sha256sum "${ZIP_NAME}" > "${ZIP_NAME}.sha256"
sha256sum "${TAR_NAME}" > "${TAR_NAME}.sha256"
cd - >/dev/null

echo "[OK] Created release artifacts in ${DIST_DIR}/:"
echo "  - ${ZIP_NAME} ($(stat -c%s "${DIST_DIR}/${ZIP_NAME}") bytes)"
echo "  - ${TAR_NAME} ($(stat -c%s "${DIST_DIR}/${TAR_NAME}") bytes)"
