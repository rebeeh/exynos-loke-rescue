#!/usr/bin/env bash
# Wrapper redirecting to package_release.sh
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${SCRIPT_DIR}/package_release.sh" "$@"
