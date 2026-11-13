# Exynos LOKE Rescue (`exynos-loke-rescue`) 📱⚡

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux-orange.svg)]()
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg?logo=rust&logoColor=white)]()
[![Release: 2.0.0](https://img.shields.io/badge/Release-2.0.0--RELEASE-green.svg)]()
[![Target: Exynos 850](https://img.shields.io/badge/Target-Samsung%20Exynos%20850-9cf.svg)]()
[![Device: Galaxy A21s](https://img.shields.io/badge/Device-Galaxy%20A21s%20(SM--A217F)-blueviolet.svg)]()

> **Linux-Native Deterministic Firmware Recovery & Partition Orchestration Engine for Samsung Exynos SoC Architecture.**  
> Bypasses Heimdall session hangs, proprietary Odin4 protocol timeouts, and `samloader-rs` dynamic `super` partition pre-flight crashes.

---

## 📑 Table of Contents

1. [Executive Overview & Problem Statement](#-executive-overview--problem-statement)
2. [Deep-Dive Protocol Analysis](#-deep-dive-protocol-analysis)
   - [Hardware Boot Chain & TEE](#hardware-boot-chain--trusted-execution-environment-tee)
   - [The `param` Partition & Emergency Recovery Flag (`0x01`)](#the-param-partition--emergency-recovery-state-0x01)
   - [Odin v3 vs Odin v4 Wire Protocol Framing](#odin-v3-vs-odin-v4-wire-protocol-framing)
   - [Root-Cause Failure Analysis of Existing Linux Utilities](#root-cause-failure-analysis-of-existing-linux-utilities)
   - [AVB 2.0, `vbmeta`, and Samsung VaultKeeper (KG State)](#android-verified-boot-avb-20-vbmeta-and-samsung-vaultkeeper-kg-state)
   - [Linux Kernel USB Subsystem Conflicts (`cdc_acm` & `ModemManager`)](#linux-kernel-usb-subsystem-conflicts-cdc_acm--modemmanager)
3. [System Architecture & Design Principles](#-system-architecture--design-principles)
   - [Zero-Copy Streaming Decompression Pipeline](#zero-copy-streaming-decompression-architecture)
   - [Deterministic State Machine for LOKE Recovery](#deterministic-state-machine-for-loke-recovery)
   - [Repository Directory Hierarchy](#repository-directory-hierarchy)
4. [Installation & Build Guide](#-installation--build-guide)
5. [Operational Runbooks](#-operational-runbooks)
   - [Runbook A: Clearing "An error has occurred..."](#runbook-a-clearing-the-blue-an-error-has-occurred-screen)
   - [Runbook B: Recovering from Post-LineageOS Dynamic `super` Bootloops](#runbook-b-recovering-from-post-lineageos-dynamic-super-bootloops)
   - [Runbook C: Resolving `LOKE FAIL! [code: -1]` & VaultKeeper Locks](#runbook-c-resolving-loke-fail-code--1--vaultkeeper-prenormal-locks)
   - [Runbook D: Clean LineageOS Installation Post-Recovery](#runbook-d-clean-lineageos-installation-post-recovery)
6. [Security, Safety & Checksum Standards](#-security-safety-and-checksum-verification-standards)
7. [Future Roadmap & Extensibility](#-future-roadmap--extensibility)

---

## 📌 Executive Overview & Problem Statement

Modern Samsung Galaxy smartphones powered by Exynos System-on-Chips (SoCs) implement a proprietary low-level flashing interface inside their secondary bootloader (`sboot.bin`), internally designated as **LOKE** (the device-side counterpart to Samsung's desktop **Odin** flashing protocol).

While Samsung provides an official Win32 graphical utility (`Odin3.exe`) for Microsoft Windows hosts, Linux systems engineers, kernel developers, and aftermarket firmware (e.g., LineageOS) maintainers rely on fragmented, reverse-engineered, or leaked command-line tools. When transitioning a device between aftermarket Android distributions (which repartition logical volumes inside Android's dynamic `super` partition) and official Samsung firmware, existing Linux utilities exhibit catastrophic failure modes that leave devices trapped in non-bootable states:

1. **Emergency Recovery Loop:** (`An error has occurred while updating the device software. Use the Emergency recovery function in the Smart Switch PC software.`)
2. **Kernel/Bootloader Signature Rejection:** (`LOKE returned general failure (FAIL!) [code: -1]`)
3. **USB Bulk Endpoint Deadlocks:** (`error: -7` / `LIBUSB_ERROR_TIMEOUT`)
4. **Dynamic Partition Metadata Crashes:** (`Dynamic partition pre-flight check failed: insufficient free space in Super partition or metadata corrupted`)

`exynos-loke-rescue` is a deterministic, Linux-native orchestration engine written in Rust. Rather than re-implementing fragile raw USB bulk-transfer bindings from scratch, it acts as an intelligent state-machine supervisor over `samloader-rs`, combining native streaming `.tar.md5` + `.lz4` decompression, binary PIT (Partition Information Table) inspection, Linux USB subsystem sanitization, and automated LOKE error heuristics.

---

## 🔬 Deep-Dive Protocol Analysis

### Hardware Boot Chain & Trusted Execution Environment (TEE)
On Exynos SoCs (exemplified by the Exynos 850 / S5E3830), cold boot proceeds through a strictly verified cryptographic chain of trust:

```
[ Mask ROM (iROM / BL1) ]
        |  Verifies RSA-2048/4096 signature of BL2
        v
[ EPBL / BL2 (sboot stage 1) ]
        |  Initializes DRAM controller, clocks, and TrustZone (TEEGRIS)
        v
[ EL3 Monitor & Bootloader (`sboot.bin` / LK - Little Kernel) ]
        |  Reads `param` partition flags, checks physical key matrix
        |  (Vol Up + Vol Down), initializes USB 2.0 High-Speed DWC3/OTG
        |  controller, and enters LOKE Download Mode if requested or flagged.
        v
[ Android Verified Boot (`vbmeta.img`) -> `boot.img` / `recovery.img` ]
```

### The `param` Partition & Emergency Recovery State (`0x01`)
Samsung Exynos devices store persistent bootloader state variables inside a dedicated raw block partition named `param` (or `up_param`). Among these fields is the **Download Mode / Update Status Flag**:

* **`0x00` (Normal Boot):** Bootloader proceeds to verify `vbmeta` and load `boot.img` (or `recovery.img` if physical recovery keys are held).
* **`0x01` (Emergency Recovery / Incomplete Update):** Set automatically by `sboot` the instant a LOKE flashing session begins (`Session begun`). This flag is **ONLY** cleared to `0x00` when the host transmits a valid **End-of-Session (`0x66` / `SESSION_END`)** packet **AND** the bootloader validates all written cryptographic footers.

If a flashing tool crashes, times out on USB, or fails to send the exact protocol-versioned `SESSION_END` packet, the `0x01` flag remains burned into non-volatile flash. On every subsequent power cycle—even after a battery disconnect—`sboot` aborts normal boot and renders the blue Emergency Recovery screen.

### Odin v3 vs. Odin v4 Wire Protocol Framing
Samsung's LOKE protocol operates over USB Bulk Endpoints (Vendor ID `0x04e8`, Product ID `0x685d`, Interface `1`, `EP_IN = 0x81`, `EP_OUT = 0x01`, `wMaxPacketSize = 512` bytes):

1. **Handshake Phase:**
   * Host sends 4-byte ASCII string: `0x4F 0x44 0x49 0x4E` (`ODIN`)
   * Device responds with 4-byte ASCII string: `0x4C 0x4F 0x4B 0x45` (`LOKE`)
2. **Control & Payload Frames:**
   * LOKE packets are fixed-size 1024-byte control frames containing little-endian 32-bit opcodes:
     * `0x64` (`CMD_SESSION_SETUP`): Negotiates protocol version (`v3` vs `v4`) and packet chunk size (`131072` bytes in v3; up to `1048576` bytes in v4).
     * `0x65` (`CMD_PIT_TRANSFER`): Requests or uploads the binary PIT table.
     * `0x66` (`CMD_FILE_TRANSFER`): Initiates partition flash (`START_PART`), streams data chunks, and finalizes partition (`END_PART`).
     * `0x67` (`CMD_SESSION_END`): Commits `param` state to `0x00` and triggers warm reboot (`0x01` normal, `0x02` download mode).

### Root-Cause Failure Analysis of Existing Linux Utilities

| Utility | Failure Mechanism | Root Cause |
| :--- | :--- | :--- |
| **Heimdall v2.2.2** | Uploads images to 100%, then prints: `Failed to receive session end confirmation!` | Heimdall was engineered around older Galaxy S2–S10 (Odin v2/v3) bootloaders. On modern Exynos chips (Exynos 850), `sboot` expects updated payload struct fields in `CMD_SESSION_END`. Rejecting the legacy frame leaves `param` flag `0x01` intact. |
| **Leaked `odin4` (Linux)** | Handshake succeeds, followed by five consecutive `USB bulk read failed (error: -7)` timeouts at 10s intervals. | `odin4` unconditionally issues Odin v4 device-type query packets (`subcommand 0x05`) after handshake. Exynos 850 bootloaders do not implement this subcommand; `sboot` silently drops it, causing `libusb_bulk_transfer()` on `EP_IN (0x81)` to block until timeout (`-7`). |
| **`samloader-rs` (Full AP)** | `ERROR: Dynamic partition pre-flight check failed: insufficient free space in Super partition or metadata corrupted.` | Custom ROMs (e.g. LineageOS) modify logical partition geometry (`lp_metadata`). In `samloader-rs`, the archive handler invokes a pre-flight validator before evaluating `--skip-size-check` or `-e` and aborts prior to writing any blocks. |

**The Bypass:** By streaming `boot.img.lz4`, `vbmeta.img.lz4`, `recovery.img.lz4`, `dtbo.img.lz4`, and `super.img.lz4` out of `AP_*.tar.md5`, decompressing them on the host, and passing them via explicit `-p <PART> <FILE>` arguments, `samloader-rs` treats each image as a raw block payload and bypasses the broken dynamic metadata parser completely.

### Android Verified Boot (AVB 2.0), `vbmeta`, and Samsung VaultKeeper (KG State)
Samsung layers two independent verification mechanisms over standard Android Verified Boot (AVB 2.0):
1. **Samsung Signer Footer:** Every official partition image carries an appended Samsung cryptographic signature block (`SEANDROIDENFORCE` + RSA signature).
2. **VaultKeeper (`KG State` / Knox Guard):** Even after bootloader unlock, if the device enters an unbootable state or has its `param`/`misc` state disrupted without completing an Android boot cycle, VaultKeeper can transition `KG State` to `Prenormal`. While `Prenormal`, LOKE accepts uploads to DRAM but rejects session close if unsigned custom binaries (TWRP, Lineage Recovery) were flashed, aborting with `LOKE returned general failure (FAIL!) [code: -1]`. Booting official signed stock binaries into One UI with an active network connection resets VaultKeeper to `Checking`/`Completed`.

### Linux Kernel USB Subsystem Conflicts (`cdc_acm` & `ModemManager`)
When a Samsung device enumerates in Download Mode (`04e8:685d`), its USB configuration descriptor advertises a Communications Device Class Abstract Control Model (`CDC-ACM`) interface class (`0x02`):
* The Linux kernel automatically binds the `cdc_acm` driver, creating `/dev/ttyACM0`.
* User-space `ModemManager` immediately opens `/dev/ttyACM0` and transmits `AT` modem probe strings (`AT+GCAP\r\n`).
* LOKE interprets `AT` bytes as corrupted Odin framing, stalling the USB endpoint or triggering `LIBUSB_ERROR_BUSY` when flashing tools attempt `claim_interface(1)`.
* **Fix:** `ENV{ID_MM_DEVICE_IGNORE}="1"` in udev permanently instructs `ModemManager` to ignore the device.

---

## 🏗️ System Architecture & Design Principles

### Zero-Copy Streaming Decompression Architecture
Official Samsung `AP_*.tar.md5` archives routinely exceed 4.2 GiB to 6.5 GiB, containing compressed LZ4 frames (`super.img.lz4` alone expands to ~4.8 GiB). Unpacking an entire archive via shell `tar` + `lz4` requires >12 GiB of disk I/O.

`exynos-loke-rescue` implements a single-pass streaming pipeline:
```
[ AP_*.tar.md5 (Disk) ]
        |  16 MiB BufReader
        v
[ `tar::Archive` Entry Iterator ]
        |  Matches target entry (`*.img.lz4`), ignores trailing MD5 footer
        v
[ `lz4_flex::frame::FrameDecoder` ]
        |  Decompresses LZ4 frames directly in CPU cache
        v
[ 16 MiB BufWriter -> `TempDir/<partition>.img` ]
```

### Deterministic State Machine for LOKE Recovery
Every flash operation executes inside a guarded state machine:
* **State 0: `USB_SANITIZE`** $\rightarrow$ Enforce udev rules (`ID_MM_DEVICE_IGNORE`), stop ModemManager, unload `cdc_acm`.
* **State 1: `BUS_VERIFY`** $\rightarrow$ Confirm `04e8:685d` presence and query LOKE `detect`.
* **State 2: `PIT_VALIDATE`** $\rightarrow$ Binary PIT parsing (132-byte entry layout) to verify partition offsets/sizes.
* **State 3: `STREAM_EXTRACT`** $\rightarrow$ Single-pass extraction of required signed `.img` files.
* **State 4: `LOKE_FLASH`** $\rightarrow$ Invoke `samloader-rs flash -p ...` with live stdout/stderr tee.
* **State 5: `POST_MORTEM`** $\rightarrow$ If exit != 0, parse LOKE/USB error signatures and emit remediation.

### Repository Directory Hierarchy
```text
exynos-loke-rescue/
├── Cargo.toml                      # Package metadata & build profile
├── LICENSE                         # MIT License
├── README.md                       # Complete technical specification & runbooks
├── .gitignore                      # Ignore build artifacts, archives, images
├── udev/
│   └── 60-exynos-loke-rescue.rules # Kernel udev rules with ID_MM_DEVICE_IGNORE
├── scripts/
│   ├── install.sh                  # Host bootstrapper (deps, samloader-rs, cargo)
│   ├── emergency_unbrick.sh        # One-shot rescue wrapper
│   ├── 01_setup_usb.sh             # Standalone bash USB sanitizer
│   ├── 02_extract_img.sh           # Standalone bash lz4 extractor (--full supported)
│   ├── 03_flash_signed.sh          # Standalone bash boot component flasher
│   └── 04_flash_stock_os.sh        # Standalone bash raw SUPER & DTBO flasher
├── src/
│   ├── main.rs                     # Primary CLI coordinator
│   ├── cli.rs                      # Clap v4 command & argument definitions
│   ├── error.rs                    # Structured error taxonomy (thiserror)
│   ├── usb.rs                      # USB bus scanner, udev manager, and detect
│   ├── pit.rs                      # Samsung binary PIT table parser
│   ├── archive.rs                  # Streaming .tar.md5 and .lz4 decompressor
│   ├── diagnostics.rs              # LOKE return code and VaultKeeper heuristics
│   └── flasher.rs                  # samloader-rs process supervisor
└── tests/
    └── integration_tests.rs        # Synthetic tar.md5 and PIT parser tests
```

---

## 📦 Installation & Build Guide

### Automated Installation (Fedora, Debian, Ubuntu, Arch)
```bash
git clone https://github.com/rebeeh/exynos-loke-rescue.git
cd exynos-loke-rescue
./scripts/install.sh
```

### Manual Build from Source
```bash
# 1. Install toolchain
sudo dnf install -y rust cargo curl unzip usbutils lz4   # Fedora
# sudo apt install -y cargo rustc curl unzip usbutils liblz4-tool  # Ubuntu/Debian

# 2. Install samloader-rs
curl -s https://api.github.com/repos/topjohnwu/samloader-rs/releases/latest \
  | grep "browser_download_url.*linux.*x86_64" \
  | cut -d '"' -f 4 \
  | xargs curl -L -o samloader-rs.tar.gz
tar -xzf samloader-rs.tar.gz && chmod +x samloader
sudo mv samloader /usr/local/bin/

# 3. Build optimized release binary
cargo build --release
sudo cp target/release/exynos-loke-rescue /usr/local/bin/

# 4. Install udev rules
sudo cp udev/60-exynos-loke-rescue.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

---

## 📖 Operational Runbooks

### Runbook A: Clearing the Blue "An error has occurred..." Screen
1. Connect the Samsung Exynos device via a direct motherboard USB-A 2.0 cable.
2. Sanitize the Linux USB stack:
   ```bash
   exynos-loke-rescue prep-usb
   ```
3. Inspect the connected device to verify `04e8:685d` communication:
   ```bash
   exynos-loke-rescue inspect
   ```
4. Download matching official firmware (if not already cached locally):
   ```bash
   exynos-loke-rescue fetch-stock --out-dir ~/Downloads/stock
   ```
5. Extract and flash **ONLY** the signed boot chain (`BOOT`, `VBMETA`, `RECOVERY`):
   ```bash
   exynos-loke-rescue rescue-boot --ap ~/Downloads/stock
   ```
6. **Expected Result:** LOKE validates Samsung signature footers, commits `param` flag `0x00`, prints `Rebooting device...`, and exits the Emergency Recovery screen.

---

### Runbook B: Recovering from Post-LineageOS Dynamic `super` Bootloops
1. If the device clears the Emergency Recovery screen but bootloops on the Samsung logo, the `super` partition still holds custom ROM logical partition metadata incompatible with the stock kernel.
2. Force-enter Download Mode:
   * Hold **Volume Down + Power** for 8 seconds until the screen goes completely black.
   * Immediately release both and hold **Volume Up + Volume Down**.
   * Press **Volume Up** once at the teal Warning prompt.
3. Execute full raw partition extraction and flashing (bypasses `samloader-rs` pre-flight crash):
   ```bash
   exynos-loke-rescue full-raw-restore --stock-dir ~/Downloads/stock
   ```
4. When the device reboots into Samsung Stock Recovery:
   * Select **Wipe data / factory reset** $\rightarrow$ **Factory data reset**.
   * Select **Reboot system now**.
5. Wait 5–8 minutes for One UI first-boot compilation.

---

### Runbook C: Resolving `LOKE FAIL! [code: -1]` & VaultKeeper Prenormal Locks
1. If flashing custom `recovery.img` + `vbmeta.img` terminates with `LOKE returned general failure (FAIL!) [code: -1]`, VaultKeeper has set `KG State: Prenormal`.
2. Complete **Runbook B** to boot into stock Samsung One UI.
3. Connect to Wi-Fi and wait 60 seconds for background Knox Guard check-in.
4. Navigate to `Settings > About phone > Software information`, tap `Build number` 7 times, open `Settings > Developer options`, and verify that `OEM unlocking` is ON and greyed out, and enable `USB debugging`.
5. Reboot to Download Mode:
   ```bash
   adb reboot download
   ```

---

### Runbook D: Clean LineageOS Installation Post-Recovery
1. Flash LineageOS Recovery and `vbmeta` with `--no-reboot`:
   ```bash
   exynos-loke-rescue flash-custom-recovery \
       --recovery ~/Downloads/recovery.img \
       --vbmeta ~/Downloads/vbmeta.img
   ```
2. Unplug USB, hold **Volume Down + Power** until screen turns black, then immediately hold **Volume Up + Power** (releasing `Power` at the Samsung logo while holding `Volume Up`) to enter Lineage Recovery.
3. Tap **Factory reset** $\rightarrow$ **Format data / factory reset** $\rightarrow$ **Format data**.
4. Tap back, select **Apply update** $\rightarrow$ **Apply from ADB**, and run:
   ```bash
   adb sideload ~/Downloads/lineage-*.zip
   ```
5. Tap **Reboot system now**.

---

## 🔒 Security, Safety, and Checksum Verification Standards

1. **Non-Destructive Partition Scoping:**  
   `rescue-boot` and `full-raw-restore` never touch `efs`, `sec_efs`, `cpefs`, or `nvdata` partitions, preserving device IMEI, baseband calibration, and MAC addresses across every recovery cycle.
2. **Anti-Rollback Fuse Protection:**  
   Samsung Exynos `sboot` enforces hardware eFuse rollback counters (`SW REV`) on `BL` and `AP` binaries. `fetch-stock` queries Samsung FUS servers for the latest active firmware matching the device's `Sales Code` (CSC) so downloaded images are always $\ge$ the current eFuse revision.
3. **Temporary Disk Hygiene:**  
   Decompressed multi-gigabyte images (`super.img` ~4.8 GiB) are written into RAII-guarded `tempfile::TempDir` directories that automatically unlink upon process completion or error exit unless `--keep-extracted` is explicitly passed.

---

## 🗺️ Future Roadmap & Extensibility

* **Native Rust LOKE USB Transport (`nusb` direct):** Replace subprocess delegation to `samloader-rs` with an embedded Odin v3/v4 wire-protocol driver supporting zero-copy streaming from `FrameDecoder` directly into 131072-byte USB bulk `OUT` transfers without writing `super.img` to disk.
* **Automated `lpunpack` / `lpmake` Metadata Inspector:** Read the first 1 MiB of the device's `super` partition (or inspect local `super.img`) to render exact dynamic partition extents (`system`, `vendor`, `product`, `odm`) before flashing.
* **Interactive `ratatui` Terminal Dashboard:** Real-time USB endpoint telemetry, temperature monitoring (`sboot` thermal sensor readout), and guided hardware key-combo timers for devices with damaged physical buttons.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).  
Copyright (c) 2026 Rabeeh Fouz Ahammad VK.
