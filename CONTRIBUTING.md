# Contributing to Exynos LOKE Rescue

Thank you for your interest in contributing to `exynos-loke-rescue`. This document outlines the development workflow, coding standards, and security requirements for this project.

## Architectural Principles

1. **Clean-Room Wire Protocol Separation:**
   All protocol implementations must be derived strictly from observed USB wire telemetry (`usbmon` / `tshark` captures) or public reverse-engineering analysis. No proprietary Samsung source code or decompiled binaries may be committed to this repository.

2. **Partition Safety Invariant:**
   Under no circumstances may this tool support writing to `EFS`, `SEC_EFS`, `CPEFS`, or `NVDATA` partitions. These partitions contain unique radio calibration, IMEI, and cryptographic identity keys that cannot be recovered if overwritten.

3. **Memory Safety & Zero Unsafe:**
   All application and protocol logic must adhere to safe Rust. Any direct FFI or raw memory pointer manipulation must be fully documented and validated with formal bounds checking.

## Development Workflow

### Prerequisites
* Rust 1.70+ with Cargo
* `make` (optional, for unified commands)
* `lz4` and `usbutils` (for runtime utilities)

### Common Commands
```bash
# Check code syntax and type signatures
make check

# Run unit and integration tests
make test

# Format code according to Rustfmt
make fmt

# Run Clippy linter with strict warning flags
make lint

# Run release hardening & exposure audit
make audit

# Assemble a local release distribution tarball
make package-tarball
```

## Pull Request Checklist
Before submitting a PR:
- [ ] `cargo test --all-targets` passes with 0 failures.
- [ ] `cargo clippy --all-targets -- -D warnings` reports 0 warnings.
- [ ] `cargo fmt --all --check` reports clean formatting.
- [ ] Protocol changes include wire captures or packet diagram documentation in `PROTOCOL.md`.
