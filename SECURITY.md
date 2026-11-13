# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.0.x   | :white_check_mark: |
| < 1.0   | :x:                |

## Security Model & Hardening

`exynos-loke-rescue` is executed with root or elevated USB permissions (`plugdev`) to communicate directly with bootloader USB endpoints. As such, the project employs strict defense-in-depth security hardening:

* **Memory Safety:** 100% written in safe Rust to eliminate buffer overflows, use-after-free, and pointer aliasing vulnerabilities.
* **Linker Hardening:**
  * Full Relocations (RELRO with `BIND_NOW`)
  * Stack Smashing Protection
  * Position-Independent Executable (PIE)
  * Data Execution Prevention (NX / PT_GNU_STACK without `PF_X`)
  * Stripped symbol table in release builds
* **Zero Credential Retention:** Automated static exposure audits verify that no API keys, developer paths, or debug companions are leaked into release artifacts.

## Reporting a Vulnerability

If you discover a security vulnerability within `exynos-loke-rescue`, please DO NOT open a public issue. Instead, report it privately via GitHub Security Advisories or by emailing the project maintainer at `rabeeh@users.noreply.github.com`.
