# Exynos LOKE Rescue (`exynos-loke-rescue`)

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux-orange.svg)]()
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg?logo=rust&logoColor=white)]()
[![Protocol: Clean--Room](https://img.shields.io/badge/Protocol-Clean--Room%20Odin%20v3%2Fv4-purple.svg)](PROTOCOL.md)
[![Release: 1.0.0](https://img.shields.io/badge/Release-1.0.0--ENTERPRISE-green.svg)]()
[![Target: Exynos 850](https://img.shields.io/badge/Target-Samsung%20Exynos%20850-9cf.svg)]()
[![Device: Galaxy A21s](https://img.shields.io/badge/Device-Galaxy%20A21s%20(SM--A217F)-blueviolet.svg)]()

> **Native Linux Firmware Recovery Engine, Clean-Room Odin v3/v4 Protocol Specification, and Deterministic Partition Orchestrator for Samsung Exynos SoCs.**  
> Built from protocol reverse engineering via **REA (Reverse Engineering Anything)**, Ghidra MCP analysis, and live Linux `usbmon` wire telemetry.

---

## Table of Contents

1. [Executive Overview & Architectural Mandate](#executive-overview--architectural-mandate)
2. [Clean-Room Reverse Engineering Methodology with REA](#clean-room-reverse-engineering-methodology-with-rea)
   - [Legal & Architectural Clean-Room Separation](#legal--architectural-clean-room-separation)
   - [Live USB Wire Telemetry Capture (`usbmon` + `tshark`)](#live-usb-wire-telemetry-capture-usbmon--tshark)
   - [REA & Ghidra Analysis of `odin4` & `sboot.bin`](#rea--ghidra-analysis-of-odin4--sbootbin)
   - [Differential Analysis: Odin v3 vs. Odin v4](#differential-analysis-odin-v3-vs-odin-v4)
3. [Dual-Engine Architecture](#dual-engine-architecture)
   - [Engine 1: 100% Native Rust LOKE Transport (`nusb`)](#engine-1-100-native-rust-loke-transport-nusb)
   - [Engine 2: Zero-Copy Archive Streamer & Supervisor](#engine-2-zero-copy-archive-streamer--supervisor)
4. [Repository Hierarchy](#repository-hierarchy)
5. [Installation & Setup](#installation--setup)
6. [Protocol Telemetry & Dissection](#protocol-telemetry--dissection)
7. [CLI Command Reference](#cli-command-reference)
8. [Operational Runbooks](#operational-runbooks)
   - [Runbook A: Clearing "An error has occurred..."](#runbook-a-clearing-the-blue-an-error-has-occurred-screen)
   - [Runbook B: Recovering from Post-LineageOS Dynamic `super` Bootloops](#runbook-b-recovering-from-post-lineageos-dynamic-super-bootloops)
   - [Runbook C: Resolving `LOKE FAIL! [code: -1]` & VaultKeeper Locks](#runbook-c-resolving-loke-fail-code--1--vaultkeeper-prenormal-locks)
   - [Runbook D: Clean LineageOS Installation Post-Recovery](#runbook-d-clean-lineageos-installation-post-recovery)
   - [Runbook E: Resolving "Dynamic partition pre-flight check failed"](#runbook-e-resolving-the-dynamic-partition-pre-flight-check-failed-error)
9. [Security & Partition Safety Invariants](#security-and-partition-safety-invariants)

---

## Executive Overview & Architectural Mandate

Modern Samsung Galaxy smartphones powered by Exynos System-on-Chips (SoCs) implement a proprietary low-level flashing interface inside their secondary bootloader (`sboot.bin`), internally named **LOKE** (the device-side counterpart to Samsung's desktop **Odin** flashing protocol).

Across GNU/Linux environments, developers and custom ROM maintainers (such as LineageOS builders) face severe reliability barriers when flashing Exynos devices:

1. **The Odin v4 `-7` Bulk Timeout:** Leaked Linux `odin4` binaries unconditionally issue Odin v4 device-type query packets (`CMD_SESSION_SETUP` subcommand `0x05`) immediately after the initial handshake. Exynos 850 (`S5E3830`) bootloaders only implement Odin v3, silently dropping the v4 query and triggering `USB bulk read failed (error: -7)` timeouts.
2. **Heimdall Session Termination Hang:** `Heimdall v2.2.2` speaks Odin v3 framing but emits an outdated `SESSION_END` control structure that modern Exynos `sboot` bootloaders reject, leaving the bootloader's persistent `param` update flag burned at `0x01` (Emergency Recovery Loop).
3. **`samloader-rs` Dynamic Partition Crash:** `samloader-rs` speaks valid LOKE framing, but its `AP` tarball handler enforces a pre-flight `super` partition geometry check that crashes whenever leftover custom ROM logical partition extents are present—even with `--skip-size-check`.

`exynos-loke-rescue` permanently solves all three issues:
* **Native Rust LOKE Transport (`nusb`):** Directly implements the clean-room Odin v3/v4 wire protocol over raw USB bulk endpoints with adaptive protocol negotiation, requiring **zero external binaries**.
* **Zero-Copy Streaming Decompression:** Streams partition payloads (`boot`, `vbmeta`, `recovery`, `dtbo`, `super`) directly out of multi-gigabyte `AP_*.tar.md5` archives and flashes them via targeted partition commands (`-p`), completely bypassing dynamic partition calculation bugs.

---

## Clean-Room Reverse Engineering Methodology with REA

### Legal & Architectural Clean-Room Separation
To guarantee clean intellectual property boundaries and strict MIT License compatibility, all protocol specifications were produced via a two-phase clean-room process:
* **Phase I (Protocol Specification - [`PROTOCOL.md`](PROTOCOL.md)):** Developed using **REA (Reverse Engineering Anything)**, Ghidra MCP decompilation of unstripped `odin4` symbols, and passive Linux kernel `usbmon` packet telemetry. Contains abstract wire frame definitions, offsets, and state machine diagrams.
* **Phase II (Clean-Room Implementation):** Implemented in memory-safe Rust (`src/loke_proto.rs` and `src/nusb_transport.rs`) referencing only [`PROTOCOL.md`](PROTOCOL.md).

### Live USB Wire Telemetry Capture (`usbmon` + `tshark`)
Live packet captures of official Odin sessions were captured on Linux using the kernel `usbmon` facility:
```bash
# Capture USB bulk frames during active session
./scripts/capture_usbmon.sh ~/loke_capture.pcapng
```

### REA & Ghidra Analysis of `odin4` & `sboot.bin`
Using REA's Ghidra integration to analyze target artifacts:
1. **Host Reference (`odin4`):** Disassembly of `libusb_bulk_transfer` wrappers mapped control frame opcodes (`0x64`–`0x67`), revealing that `odin4` sends subcommand `0x05` unconditionally.
2. **Device Reference (`sboot.bin`):** Decompilation of the ARM64 Little Kernel (LK) command switch loop at base address `0x8F000000` revealed:
   * `case 0x64`: Rejection of unknown subcommands (`0x05` returns zero bytes).
   * `case 0x66`: Appended signature verification (`SEANDROIDENFORCE`) and the exact logic that returns `[0xFFFFFFFF, -1]` when `KG State: Prenormal` rejects unsigned custom images.
   * `case 0x67`: Commits `param` partition flag from `0x01` to `0x00`.

To prepare `sboot.bin` for REA / Ghidra analysis:
```bash
./scripts/rea_extract_sboot.sh stock_fw/BL_*.tar.md5
```

### Differential Analysis: Odin v3 vs. Odin v4

| Step | Odin v3 (Exynos 850 / Galaxy A21s) | Odin v4 (Exynos 2100+ / Modern Flagships) |
| :--- | :--- | :--- |
| **1. Handshake** | Host $\rightarrow$ `"ODIN"` (4B) | Host $\rightarrow$ `"ODIN"` (4B) |
| **2. Response** | Device $\rightarrow$ `"LOKE"` (4B) | Device $\rightarrow$ `"LOKE"` (4B) |
| **3. Session Init** | Host $\rightarrow$ `[0x64, 0x00, 0x04...]` (1024B) | Host $\rightarrow$ `[0x64, 0x00, 0x04...]` (1024B) |
| **4. Negotiation** | Device $\rightarrow$ `[0x64, 0x03...]` (Negotiated v3) | Device $\rightarrow$ `[0x64, 0x04...]` (Negotiated v4) |
| **5. Query Device** | **SKIPPED** *(Sending causes error: -7 timeout!)* | Host $\rightarrow$ `[0x64, 0x05...]` (Query Device Type) |
| **6. Max Packet** | `131,072` Bytes (128 KiB bulk transfers) | `1,048,576` Bytes (1 MiB bulk transfers) |

---

## Dual-Engine Architecture

```mermaid
flowchart TD
    User["User CLI Command\n(rescue-boot / full-raw-restore)"] --> EngineChoice{"Transport Engine Selection\n(--transport native | samloader)"}

    EngineChoice -->|"Default: --transport native"| NativeEngine["Native Rust LOKE Engine (nusb)\nDirect USB Bulk OUT/IN 04e8:685d"]
    EngineChoice -->|"Fallback: --transport samloader"| SamloaderEngine["samloader-rs Subprocess\nProcess Supervisor with Pipe Tee"]

    NativeEngine --> Handshake["ODIN <-> LOKE Handshake"]
    Handshake --> ProtoDetect{"Adaptive Protocol Negotiation\n(CMD_SESSION_SETUP 0x64)"}
    
    ProtoDetect -->|"Negotiated v3 (Exynos 850)"| OdinV3["Odin v3 Mode\n128 KiB Packets\nSkip Subcmd 0x05 (No -7 Timeout)"]
    ProtoDetect -->|"Negotiated v4 (Exynos 2100+)"| OdinV4["Odin v4 Mode\n1 MiB Packets\nSend Subcmd 0x05"]

    OdinV3 --> Streamer["Zero-Copy Archive Streamer (archive.rs)\nExtracts & Decompresses LZ4 frames in memory"]
    OdinV4 --> Streamer

    Streamer --> FlashLoop["LOKE Partition Flash (0x66)\nStreams chunks with live progress"]
    FlashLoop --> SessionClose["Session End (0x67)\nCommits param flag 0x01 -> 0x00\nClears Emergency Screen!"]
```

---

## Repository Hierarchy

```text
exynos-loke-rescue/
├── Cargo.toml                      # Package metadata, pure Rust dependencies & release profile
├── Cargo.lock                      # Deterministic dependency lockfile
├── build.rs                        # Compile-time shell completions & man page generator
├── Makefile                        # Unified development, build, packaging & test driver
├── Dockerfile                      # Multi-stage lightweight OCI container (< 15 MB)
├── LICENSE                         # MIT License
├── PROTOCOL.md                     # Clean-room Odin v3/v4 wire protocol specification
├── README.md                       # Comprehensive reference manual & runbooks
├── CHANGELOG.md                    # Keep a Changelog compliant release history
├── CONTRIBUTING.md                 # Development & contribution guidelines
├── SECURITY.md                     # Security policy & binary hardening documentation
├── install.sh                      # Universal single-command installer
├── udev/
│   └── 60-exynos-loke-rescue.rules # Kernel udev rules with ID_MM_DEVICE_IGNORE
├── tools/
│   └── wireshark/                  # Wireshark / TShark Lua dissector for LOKE wire protocol
├── scripts/
│   ├── package_release.sh          # Standalone musl release packager (.zip and .tar.gz)
│   ├── audit_release.sh            # REA release binary hardening & exposure audit
│   ├── capture_usbmon.sh           # Live Odin wire telemetry capture via usbmon
│   └── rea_extract_sboot.sh        # Unpacks sboot.bin for REA / Ghidra analysis
├── src/
│   ├── lib.rs                      # Core engine library crate interface
│   ├── main.rs                     # Primary CLI coordinator & state machine
│   ├── cli.rs                      # Clap v4 argument parser & subcommands
│   ├── error.rs                    # Structured error taxonomy (RescueError)
│   ├── usb.rs                      # Direct /sys/bus/usb/devices scanner & sanitizer
│   ├── loke_proto.rs               # Clean-room Odin v3/v4 wire framing & structs
│   ├── nusb_transport.rs           # Direct native USB bulk I/O engine via nusb
│   ├── pit.rs                      # Samsung binary PIT table parser (132-byte layout)
│   ├── archive.rs                  # Zero-copy .tar.md5 and .lz4 frame streamer
│   ├── sparse.rs                   # Android Sparse Image (0xed26ff3a) parser & geometry validator
│   ├── image_verifier.rs           # TAR.MD5 checksum validator & partition sizing verifier
│   ├── mock.rs                     # Virtual in-memory Samsung LOKE device simulator
│   ├── diagnostics.rs              # Real-time LOKE error pattern interpreter
│   └── flasher.rs                  # Dual-backend native + samloader execution supervisor
└── tests/
    ├── protocol_and_archive_tests.rs # Test suite for control frames, PIT, and tar.md5
    ├── mock_device_tests.rs          # End-to-end integration tests using in-memory mock LOKE device
    └── sparse_and_verifier_tests.rs  # Unit tests for sparse images, MD5 trailers & PIT geometry
```

---

## Installation & Setup

Like `samloader-rs`, `exynos-loke-rescue` is distributed as portable standalone static musl binaries with zero glibc or dynamic C runtime dependencies. It runs on any Linux distribution without requiring complex package setup.

### Option 1: Universal 1-Line Installer (Recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/rebeeh/exynos-loke-rescue/main/install.sh | bash
```
* Automatically detects your Linux CPU architecture (`x86_64` or `aarch64`).
* Fetches the static musl prebuilt binary and installs it to `~/.local/bin/exynos-loke-rescue`.
* Requires **zero root permissions** to install.

### Option 2: Cargo (Identical to samloader-rs)
If you have a working Rust toolchain installed:
```bash
cargo install --git https://github.com/rebeeh/exynos-loke-rescue
```

### Option 3: Prebuilt Standalone Releases
Download prebuilt static musl archives from the [latest GitHub release](https://github.com/rebeeh/exynos-loke-rescue/releases/latest):
* `exynos-loke-rescue-v1.0.0-linux-x86_64.zip`
* `exynos-loke-rescue-v1.0.0-linux-aarch64.zip`

Extract and place anywhere on your PATH:
```bash
unzip exynos-loke-rescue-v1.0.0-linux-x86_64.zip
chmod +x exynos-loke-rescue
mv exynos-loke-rescue ~/.local/bin/
```

### USB Permissions Setup (`fix-usb`)
Identical to `sudo samloader fix-usb`, configure Linux udev rules to grant your user permission to flash Samsung Download Mode devices (`04e8:685d`):
```bash
sudo exynos-loke-rescue fix-usb
```

### OCI Container (Optional)
Run without installing local binaries via USB passthrough:
```bash
podman run --rm -it --privileged \
    -v /dev/bus/usb:/dev/bus/usb \
    -v "$HOME/Downloads/stock:/firmware:ro" \
    ghcr.io/rebeeh/exynos-loke-rescue:latest probe
```

---

## Protocol Telemetry & Dissection

### Wireshark / TShark Lua Dissector
Analyze USB bulk packets in real time or offline from captured `.pcapng` traces using the bundled LOKE protocol dissector:
```bash
# 1. Capture USB bulk traffic on download mode interface (04e8:685d)
./scripts/capture_usbmon.sh ~/loke_wire_trace.pcapng

# 2. Inspect decoded LOKE control frames, commands, and ACKs in TShark
tshark -X lua_script:tools/wireshark/exynos_loke.lua -r ~/loke_wire_trace.pcapng -Y "loke"
```

### Security & Binary Hardening Verification (REA)
Verify release binaries against static exposure, credential markers, and ELF security flags:
```bash
make audit
```

---

## CLI Command Reference

### 1. Sanitize USB Stack
```bash
exynos-loke-rescue prep-usb
```
* Pauses `ModemManager` and configures `ID_MM_DEVICE_IGNORE="1"`.
* Unloads conflicting `cdc_acm` kernel driver.
* Verifies presence of Samsung Download Mode (`04e8:685d`).

### 2. Probe Device & Negotiate Protocol (Native Engine)
```bash
exynos-loke-rescue probe
```
* Performs native `ODIN` $\leftrightarrow$ `LOKE` handshake.
* Negotiates protocol tier (`OdinV3` vs `OdinV4`).
* Dumps and prints the live device Partition Information Table (PIT).

### 3. Fetch Official Stock Firmware
```bash
exynos-loke-rescue fetch-stock -m SM-A217F -r INS --out-dir ~/Downloads/stock
```

### 4. Mode 1: Clear Emergency Recovery Screen (Unbrick)
```bash
exynos-loke-rescue rescue-boot --ap ~/Downloads/stock
```
* Streams `boot.img`, `vbmeta.img`, and `recovery.img` directly from `AP_*.tar.md5`.
* Flashes signed components to satisfy Knox integrity checks.
* Sends `CMD_SESSION_END (0x67)` to commit `param` flag `0x01` $\rightarrow$ `0x00`.

### 5. Mode 2: Full Raw Stock Restore (Bypasses Super Metadata Bugs)
```bash
exynos-loke-rescue full-raw-restore --stock-dir ~/Downloads/stock
```
* Flashes `BOOT`, `VBMETA`, `RECOVERY`, `DTBO`, and `SUPER` partition-by-partition.
* Completely bypasses `samloader-rs`'s dynamic partition pre-flight crash.
* Supports `-e, --erase` to wipe USERDATA at the hardware level before flashing.

### 6. Mode 3: Direct Raw Partition Flashing (`flash-raw`)
```bash
# Flash arbitrary partition files directly:
exynos-loke-rescue flash-raw --super ~/Downloads/stock/super.img --dtbo ~/Downloads/stock/dtbo.img

# Or using explicit partition names:
exynos-loke-rescue flash-raw -p SUPER super.img -p DTBO dtbo.img --skip-size-check
```

### 7. Mode 4: Hardware NAND Erase (`erase-userdata` / `emergency-wipe-reboot`)
```bash
# Erase USERDATA at the hardware NAND controller layer and auto-reboot to One UI:
exynos-loke-rescue erase-userdata

# Or single-command unbrick for broken power button recovery bypass:
exynos-loke-rescue emergency-wipe-reboot
```
* Issues LOKE command `0x64:0x07` (hardware NAND erase of USERDATA/cache).
* Bypasses the broken power-button limitation where the user cannot confirm the stock recovery factory reset prompt!

### 8. Archive & Image Geometry Verification (`verify-image`)
```bash
# Verify official Samsung .tar.md5 checksum trailer and inspect partition contents:
exynos-loke-rescue verify-image -i ~/Downloads/stock/AP_*.tar.md5 -p ~/Downloads/stock/A21S_EUR_OPEN.pit

# Deep inspect Android Sparse Image geometry on super.img against device PIT boundaries:
exynos-loke-rescue verify-image -i ~/Downloads/stock/super.img -p ~/Downloads/stock/A21S_EUR_OPEN.pit
```
* Validates trailing 32-character hexadecimal MD5 checksum against the uncorrupted TAR stream.
* Identifies Android Sparse image magic (`0xed26ff3a`), computing total unsparse blocks and expanded byte size.
* Compares effective partition payloads directly against PIT boundaries to catch oversized partitions before touching flash.

### 9. In-Memory Software Simulation (`simulate-flash`)
```bash
# Perform end-to-end dry-run flashing simulation against in-memory mock LOKE device:
exynos-loke-rescue simulate-flash -e

# Test fault diagnosis under simulated VaultKeeper Prenormal lock (Code -1):
exynos-loke-rescue simulate-flash --fault-vaultkeeper
```
* Emulates the complete Samsung Download Mode Little Kernel (LK) state machine in memory.
* Tests packet sequencing, handshake, NAND erase, chunked streaming, and session commitment without needing physical USB hardware.

---

## Operational Runbooks

### Runbook A: Clearing the Blue "An error has occurred..." Screen
1. Connect device via USB-A 2.0 cable to host.
2. Run `exynos-loke-rescue prep-usb`.
3. Run `exynos-loke-rescue probe` to verify communication.
4. Execute `exynos-loke-rescue rescue-boot --ap ~/Downloads/stock`.
5. Device verifies signatures, resets `param` flag to `0x00`, and exits the emergency loop.

### Runbook B: Recovering from Post-LineageOS Dynamic `super` Bootloops
1. If the device clears the emergency screen but bootloops on the Samsung logo:
   * Hold **Volume Down + Power** for 8 seconds until the screen goes black.
   * Immediately hold **Volume Up + Volume Down** with USB connected to enter Download Mode.
2. Run `exynos-loke-rescue full-raw-restore --stock-dir ~/Downloads/stock -e`.
3. Device automatically erases corrupt leftover userdata and reboots directly into One UI.

### Runbook C: Resolving `LOKE FAIL! [code: -1]` & VaultKeeper Prenormal Locks
1. If custom recovery flashing is rejected with `[code: -1]`, VaultKeeper has locked `KG State` to `Prenormal`.
2. Boot into stock One UI (Runbook B).
3. Connect to Wi-Fi and wait 60 seconds for Knox Guard background sync.
4. Go to **Settings $\rightarrow$ Developer options** and confirm **OEM unlocking** is ON and greyed out.
5. Reboot to Download Mode (`adb reboot download`).

### Runbook D: Clean LineageOS Installation Post-Recovery
1. Flash Lineage Recovery and `vbmeta` with `--no-reboot`:
   ```bash
   exynos-loke-rescue flash-custom-recovery \
       --recovery ~/Downloads/recovery.img \
       --vbmeta ~/Downloads/vbmeta.img
   ```
2. Unplug USB, hold **Volume Down + Power** until black, then immediately hold **Volume Up + Power** to enter Lineage Recovery.
3. Select **Factory reset $\rightarrow$ Format data/factory reset**.
4. Select **Apply update $\rightarrow$ Apply from ADB**, and sideload:
   ```bash
   adb sideload ~/Downloads/lineage-*.zip
   ```

### Runbook E: Resolving the "Dynamic partition pre-flight check failed" Error

If you see this error when flashing with samloader:
```text
ERROR: Dynamic partition pre-flight check failed: insufficient free space in Super partition
or metadata corrupted. When flashing HOME_CSC without wipe, the Super partition must have
enough space for the target OS image. (Try flashing the CSC binary to perform a clean flash)
```

**Root Cause:**
`samloader-rs` attempts to parse Android `liblp` dynamic partition metadata on your device and inside `super.img` to ensure all logical partitions fit. When recovering from a corrupted or custom partition table (e.g. LineageOS `super`), this client-side pre-flight assertion fails and halts the session before transmission.

**Solution 1: Bypass Pre-flight Check with samloader (`--skip-size-check`)**
Include `--skip-size-check` and pass the PIT file to re-partition:
```bash
./samloader flash \
    --skip-size-check \
    --pit ~/Downloads/stock/A21S_EUR_OPEN.pit \
    -p SUPER ~/Downloads/stock/super.img \
    -p DTBO ~/Downloads/stock/dtbo.img
```

**Solution 2: Use Native `exynos-loke-rescue` (Bypasses Pre-Flight Checks Automatically)**
`exynos-loke-rescue` writes raw blocks directly to the flash controller without brittle client-side metadata constraints:
```bash
exynos-loke-rescue flash-raw \
    --super ~/Downloads/stock/super.img \
    --dtbo ~/Downloads/stock/dtbo.img \
    --skip-size-check
```

### Runbook F: Bypassing Stock Recovery Factory Reset When Physical Power Button Is Broken

If your device is stuck in a bootloop displaying:
$$\text{"Cannot load Android system. Your data may be corrupt. Try factory data reset."}$$
...and your physical Power button is physically broken or non-functional, you cannot confirm menu options in Samsung Stock Recovery because it only maps `KEY_POWER` as the confirmation trigger.

**Solution:**
1. Boot into Download Mode without the power key: Allow the battery to drain until the screen shuts off completely. Plug into a wall charger for 5 minutes, then unplug.
2. Hold **Volume Up + Volume Down** simultaneously, and plug the USB cable connected to your PC into the phone.
3. When the teal Warning screen appears, press **Volume Up** once.
4. Execute:
   ```bash
   exynos-loke-rescue emergency-wipe-reboot
   ```
5. `exynos-loke-rescue` transmits opcode `0x64:0x07` (hardware NAND format of USERDATA) and `0x67:0x01` (reboot). The phone restarts straight into One UI, completely bypassing Stock Recovery!
```bash
exynos-loke-rescue full-raw-restore --stock-dir ~/Downloads/stock
```

---

## Security and Partition Safety Invariants

* **IMEI & Baseband Invariance:** Partitions `EFS`, `SEC_EFS`, `CPEFS`, and `NVDATA` are strictly protected from modification.
* **PIT Boundary Validation:** `nusb_transport.rs` cross-references every image against `PITEntry.size_bytes()` before transmitting over USB.
* **eFuse Anti-Rollback Protection:** `fetch-stock` pulls active firmware $\ge$ the hardware fuse revision to prevent `SW REV. CHECK FAIL`.

---

## License

This project is licensed under the [MIT License](LICENSE).  
Clean-room protocol specification derived via REA and Linux `usbmon` telemetry.  
Copyright (c) 2026 Rabeeh Fouz Ahammad VK.
