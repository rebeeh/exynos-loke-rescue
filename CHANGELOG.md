# Changelog

All notable changes to `exynos-loke-rescue` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-11-14

### Added
- Clean-room Odin v3 and Odin v4 wire protocol driver implemented natively in Rust (`src/loke_proto.rs`, `src/nusb_transport.rs`).
- Adaptive version negotiation skipping subcommand `0x05` on Odin v3 bootloaders (solving `LIBUSB_ERROR_TIMEOUT / -7` crashes).
- High-performance streaming `.tar.md5` and LZ4 frame extractor directly decompressing raw partition images into device flash without intermediate disk storage.
- Binary Samsung PIT parser with magic `0x12349876` validation and sector-to-byte geometry calculation (`src/pit.rs`).
- Dynamic partition size check bypass (`--skip-size-check`) bypassing client-side metadata preflight crashes on custom ROM logical partitions.
- Automated Linux USB sanitizer (`prep-usb`) managing ModemManager isolation, `cdc_acm` kernel driver unbinding, and `04e8:685d` udev rule registration.
- REA (Reverse Engineering Anything) release binary hardening and static exposure verification engine (`scripts/audit_release.sh`).
- Wireshark / TShark Lua protocol dissector for USB bulk packet captures (`tools/wireshark/exynos_loke.lua`).
- Packaging configurations for Debian (`.deb`), Red Hat (`.rpm`), Arch Linux AUR (`PKGBUILD`), and standalone portable musl archives (`.tar.gz`).
- Shell completions generator for Bash, Zsh, Fish, and Elvish via `build.rs`.
- Automated UNIX man page generator (`exynos-loke-rescue.1`).
- Multi-arch OCI Container image support (`Dockerfile`).
- Complete GitHub Actions release matrix for x86_64, aarch64, and armv7 Linux targets.

### Removed
- Legacy ad-hoc shell scripts (`scripts/01-04`, `emergency_unbrick.sh`) fully superseded by native CLI subcommands.
- Legacy Python prototype (`exynos_rescue.py`).
