#!/usr/bin/env python3
"""
ExynosRescue (odin-unbrick-cli)
==============================
Intelligent Linux recovery orchestrator for Samsung Exynos devices.
Wraps samloader-rs, lz4, and tar to bypass Heimdall timeouts, LOKE session hangs,
and dynamic partition metadata crashes.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

# --- ANSI Formatting & Visual Helpers ---
class Style:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    DIM = "\033[2m"
    RED = "\033[31m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    BLUE = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN = "\033[36m"
    WHITE = "\033[37m"

def info(msg: str):
    print(f"{Style.BLUE}[INFO]{Style.RESET} {msg}")

def success(msg: str):
    print(f"{Style.GREEN}[OK]{Style.RESET} {msg}")

def warn(msg: str):
    print(f"{Style.YELLOW}[WARN]{Style.RESET} {msg}")

def error(msg: str):
    print(f"{Style.RED}[ERROR]{Style.RESET} {msg}", file=sys.stderr)

def banner():
    print(f"{Style.CYAN}{Style.BOLD}")
    print("╔══════════════════════════════════════════════════════════════════╗")
    print("║   ExynosRescue - Samsung LOKE Recovery & Flashing Orchestrator   ║")
    print("╚══════════════════════════════════════════════════════════════════╝")
    print(f"{Style.RESET}")

# --- Diagnostic Error Interpreter ---
class DiagnosticInterpreter:
    """Interprets samloader / Odin stdout and stderr streams in real-time,
    providing human-readable explanations and actionable bypass instructions."""

    ERROR_PATTERNS = [
        (
            re.compile(r"(?:FAIL!\s*\[code:\s*-1\]|general failure)", re.IGNORECASE),
            "VaultKeeper / Knox KG State Lock Rejected Session",
            (
                "The Samsung bootloader is currently locked by VaultKeeper (KG State: Prenormal/Checking).\n"
                "  It strictly rejects session-close packets if unsigned custom images (TWRP, Lineage Recovery) were flashed.\n"
                "  -> Action: Run 'exynos-rescue rescue-boot' to flash officially signed Samsung boot components\n"
                "     to clear the LOKE Emergency Recovery state, or restore One UI to enable OEM Unlocking."
            )
        ),
        (
            re.compile(r"(?:bulk\s*read\s*failed|error:\s*-7|LIBUSB_ERROR_TIMEOUT)", re.IGNORECASE),
            "USB Bulk Read Timeout / xHCI Endpoint Hang",
            (
                "The USB endpoint on the host or phone is hung after an incomplete LOKE transaction.\n"
                "  -> Action: Unplug the USB cable, wait 5 seconds, and plug it back in.\n"
                "  -> Also run 'exynos-rescue fix-usb' to ensure ModemManager isn't intercepting the port."
            )
        ),
        (
            re.compile(r"(?:insufficient\s*free\s*space|dynamic\s*partition|pre-flight)", re.IGNORECASE),
            "Dynamic Partition Metadata Pre-flight Crash",
            (
                "samloader-rs crashed checking the 'super' partition geometry because a previous custom ROM\n"
                "  (e.g., LineageOS) scrambled the dynamic logical partition table.\n"
                "  -> Action: Do NOT flash the full AP tarball directly.\n"
                "  -> Run 'exynos-rescue rescue-boot' to unbrick, or 'exynos-rescue raw-stock' to flash raw partitions."
            )
        ),
        (
            re.compile(r"(?:permission\s*denied|LIBUSB_ERROR_ACCESS)", re.IGNORECASE),
            "Linux USB Access Permission Denied",
            (
                "Your Linux user does not have permission to open the raw Samsung USB device (04e8:685d).\n"
                "  -> Action: Run 'sudo exynos-rescue fix-usb' to write udev rules and configure access."
            )
        ),
    ]

    @classmethod
    def wrap_command(cls, cmd: list[str], task_name: str) -> int:
        info(f"Executing: {' '.join(cmd)}")
        try:
            process = subprocess.Popen(
                cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1
            )
        except FileNotFoundError:
            error(f"Command not found: {cmd[0]}")
            return 127

        detected_issues = []
        if process.stdout:
            for line in process.stdout:
                line_clean = line.rstrip()
                print(f"  {line_clean}")

                for pattern, issue_title, suggestion in cls.ERROR_PATTERNS:
                    if pattern.search(line_clean) and issue_title not in detected_issues:
                        detected_issues.append(issue_title)
                        print(f"\n{Style.RED}{Style.BOLD}🚨 Diagnostic Interpreter Detected: {issue_title}{Style.RESET}")
                        print(f"{Style.YELLOW}{suggestion}{Style.RESET}\n")

        ret = process.wait()
        if ret == 0:
            success(f"{task_name} succeeded.")
        else:
            error(f"{task_name} exited with status code {ret}.")
        return ret


# --- Environment & Tool Validation ---
def check_prerequisites(required: list[str]) -> bool:
    missing = []
    for tool in required:
        if shutil.which(tool) is None:
            missing.append(tool)

    if missing:
        error(f"Missing required utilities: {', '.join(missing)}")
        print("\nInstall required tools:")
        print("  Fedora:  sudo dnf install -y lz4 tar curl")
        print("  Ubuntu:  sudo apt install -y lz4 tar curl")
        print("  Arch:    sudo pacman -S lz4 tar curl")
        if "samloader" in missing:
            print("\nTo install topjohnwu/samloader-rs:")
            print("  curl -s https://api.github.com/repos/topjohnwu/samloader-rs/releases/latest \\")
            print("    | grep \"browser_download_url.*linux.*x86_64\" \\")
            print("    | cut -d '\"' -f 4 \\")
            print("    | xargs curl -L -o samloader-rs.tar.gz")
            print("  tar -xzf samloader-rs.tar.gz && chmod +x samloader")
            print("  sudo mv samloader /usr/local/bin/")
        return False
    return True


# --- Feature 1: USB Sanitizer ---
def cmd_fix_usb(args):
    """Sanitize USB: stop ModemManager, unload cdc_acm, install udev rules, verify USB."""
    info("Sanitizing Linux USB environment for Samsung Download Mode...")

    # Check root for system changes
    is_root = os.geteuid() == 0

    # Step 1: samloader fix-usb or fallback udev rule
    if shutil.which("samloader"):
        info("Applying samloader udev rules via 'samloader fix-usb'...")
        subprocess.run(["samloader", "fix-usb"], check=False)
    else:
        warn("samloader binary not in PATH; checking fallback udev rule.")

    udev_path = Path("/etc/udev/rules.d/99-samsung-download-mode.rules")
    if is_root:
        try:
            udev_content = (
                "# Samsung Download Mode & LOKE Protocol USB permissions\n"
                'SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", MODE="0666", GROUP="plugdev"\n'
                'SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="685d", MODE="0666", GROUP="plugdev"\n'
                'SUBSYSTEM=="usb", ATTR{idVendor}=="04e8", ATTR{idProduct}=="6860", MODE="0666", GROUP="plugdev"\n'
            )
            udev_path.write_text(udev_content)
            subprocess.run(["udevadm", "control", "--reload-rules"], check=False)
            subprocess.run(["udevadm", "trigger"], check=False)
            success(f"Wrote udev rule to {udev_path} and reloaded udev.")
        except Exception as e:
            warn(f"Could not write udev rule: {e}")
    else:
        warn(f"Not running as root; cannot write to {udev_path}. Run with sudo if you encounter permission errors.")

    # Step 2: Stop ModemManager
    if shutil.which("systemctl"):
        info("Checking ModemManager service...")
        res = subprocess.run(["systemctl", "is-active", "ModemManager"], capture_output=True, text=True)
        if res.stdout.strip() == "active":
            if is_root:
                subprocess.run(["systemctl", "stop", "ModemManager"], check=False)
                success("Stopped ModemManager service.")
            else:
                warn("ModemManager is active and might grab the Samsung serial port. Stop it via: sudo systemctl stop ModemManager")
        else:
            success("ModemManager is not interfering.")

    # Step 3: Unload conflicting cdc_acm kernel driver
    if is_root and shutil.which("modprobe"):
        lsmod = subprocess.run(["lsmod"], capture_output=True, text=True)
        if "cdc_acm" in lsmod.stdout:
            info("Unloading conflicting cdc_acm driver...")
            subprocess.run(["modprobe", "-r", "cdc_acm"], check=False)
            success("cdc_acm module unloaded.")

    # Step 4: Scan USB Bus
    if shutil.which("lsusb"):
        info("Scanning USB bus for Samsung Download Mode devices (04e8:685d)...")
        res = subprocess.run(["lsusb", "-d", "04e8:"], capture_output=True, text=True)
        if res.stdout.strip():
            success(f"Found Samsung USB device:\n  {res.stdout.strip()}")
        else:
            warn("No Samsung device found matching Vendor ID 04e8.")
            print("  Make sure your phone is plugged in and in Download Mode (teal screen) or Emergency Recovery.")

    # Step 5: samloader detect
    if shutil.which("samloader"):
        info("Testing communication with 'samloader detect'...")
        subprocess.run(["samloader", "detect"], check=False)


# --- Feature 2: Detect & Parse Device Info ---
def parse_device_info():
    """Run 'samloader detect' and parse device details."""
    if not shutil.which("samloader"):
        error("samloader is required for device detection.")
        return None

    res = subprocess.run(["samloader", "detect"], capture_output=True, text=True)
    out = res.stdout + res.stderr

    info_dict = {}
    model_match = re.search(r"Model:\s*([A-Za-z0-9_-]+)", out)
    region_match = re.search(r"(?:Region|Sales Code|CSC):\s*([A-Za-z0-9]+)", out)
    pda_match = re.search(r"(?:PDA|Bootloader Version|Software):\s*([A-Za-z0-9_-]+)", out)
    state_match = re.search(r"State:\s*([A-Za-z0-9_\s-]+)", out)

    if model_match:
        info_dict["model"] = model_match.group(1).strip()
    if region_match:
        info_dict["region"] = region_match.group(1).strip()
    if pda_match:
        info_dict["pda"] = pda_match.group(1).strip()
    if state_match:
        info_dict["state"] = state_match.group(1).strip()

    info_dict["raw_output"] = out
    return info_dict

def cmd_detect(args):
    """Detect connected Samsung device and display parsed parameters."""
    banner()
    info("Detecting connected Samsung device in Download Mode...")
    data = parse_device_info()
    if not data or not data.get("model"):
        warn("Could not parse structured device info. Raw output:")
        print(data.get("raw_output", "") if data else "No output.")
        print("\nTroubleshooting:")
        print("  1. Run 'sudo exynos-rescue fix-usb'")
        print("  2. Re-plug USB cable")
        print("  3. Verify phone screen displays Download Mode or 'An error has occurred...'")
        return 1

    print(f"\n{Style.GREEN}{Style.BOLD}Connected Device Profile:{Style.RESET}")
    print(f"  • Model:       {Style.BOLD}{data.get('model', 'Unknown')}{Style.RESET}")
    print(f"  • Sales Code:  {Style.BOLD}{data.get('region', 'Unknown')}{Style.RESET}")
    if "pda" in data:
        print(f"  • Bootloader:  {data.get('pda')}")
    if "state" in data:
        print(f"  • State:       {data.get('state')}")
    print()
    return 0


# --- Feature 3: One-Command Firmware Fetch & Cache ---
def cmd_fetch(args):
    """Check and download matching stock firmware."""
    banner()
    if not check_prerequisites(["samloader", "curl"]):
        return 1

    model = args.model
    region = args.region

    if not model or not region:
        info("Model/region not fully specified; attempting auto-detection...")
        detected = parse_device_info()
        if detected:
            model = model or detected.get("model")
            region = region or detected.get("region")

    if not model or not region:
        error("Could not auto-detect device model and region. Please specify them with --model and --region.")
        return 1

    info(f"Target Device: Model={model}, Region={region}")
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    info("Checking latest firmware available from Samsung FUS servers...")
    res = subprocess.run(["samloader", "check-update", "-m", model, "-r", region], check=False)
    if res.returncode != 0:
        error("Firmware check failed. Check your model and CSC code.")
        return 1

    fw_zip = out_dir / f"stock_{model}_{region}.zip"
    info(f"Downloading firmware to: {fw_zip}...")
    DiagnosticInterpreter.wrap_command(
        ["samloader", "download", "-m", model, "-r", region, "-o", str(fw_zip)],
        "Firmware Download"
    )

    if fw_zip.exists() and args.unzip:
        info("Extracting firmware zip archive...")
        shutil.unpack_archive(str(fw_zip), str(out_dir))
        success(f"Firmware unpacked into {out_dir}")
    return 0


# --- Feature 4: Archive Inspector & Stream Extractor ---
def find_ap_tarball(specified_path: str | None = None) -> Path | None:
    """Find the AP_*.tar.md5 or AP_*.tar archive."""
    if specified_path:
        p = Path(specified_path).resolve()
        if p.is_file():
            return p

    search_dirs = [Path("."), Path("./stock_fw"), Path("./firmware"), Path.home() / "stock_fw", Path.home() / "Downloads" / "stock"]
    for d in search_dirs:
        if d.is_dir():
            for match in d.glob("AP_*.tar*"):
                if match.is_file() and not match.name.endswith(".zip"):
                    return match
    return None

def extract_and_decompress(ap_path: Path, components: list[str], target_dir: Path) -> dict[str, Path]:
    """Extract lz4 files from AP tarball and decompress with lz4 utility."""
    target_dir.mkdir(parents=True, exist_ok=True)
    extracted_imgs = {}

    lz4_files = [f"{comp}.img.lz4" for comp in components]
    info(f"Extracting [{', '.join(lz4_files)}] from {ap_path.name}...")

    # Extract matching members using tar
    tar_cmd = ["tar", "-C", str(target_dir), "-xf", str(ap_path)] + lz4_files
    res = subprocess.run(tar_cmd, check=False)
    if res.returncode != 0:
        error("tar extraction failed. Some requested partitions may not exist in this AP archive.")
        return {}

    for comp in components:
        lz4_file = target_dir / f"{comp}.img.lz4"
        raw_img = target_dir / f"{comp}.img"
        if not lz4_file.exists():
            error(f"Missing {lz4_file.name} after tar extraction.")
            continue

        info(f"Decompressing {lz4_file.name} -> {raw_img.name}...")
        lz4_res = subprocess.run(["lz4", "-df", str(lz4_file), str(raw_img)], check=False)
        if lz4_res.returncode == 0 and raw_img.exists() and raw_img.stat().st_size > 0:
            success(f"Extracted {comp.upper()}: {raw_img.name} ({raw_img.stat().st_size / (1024*1024):.1f} MB)")
            extracted_imgs[comp.upper()] = raw_img
        else:
            error(f"Failed to decompress {lz4_file.name}")

    return extracted_imgs


# --- Feature 5: Mode 1 - Soft-Brick / LOKE Session Clear ---
def cmd_rescue_boot(args):
    """Mode 1: Extract and flash signed boot, vbmeta, and recovery to break emergency loop."""
    banner()
    if not check_prerequisites(["samloader", "tar", "lz4"]):
        return 1

    ap_path = find_ap_tarball(args.ap)
    if not ap_path:
        error("Could not find stock AP archive (AP_*.tar.md5).")
        print("Provide the path with --ap path/to/AP_*.tar.md5 or place it in the current directory.")
        return 1

    info(f"Using AP archive: {ap_path}")
    work_dir = Path(args.work_dir or "./extracted_rescue").resolve()

    # Required signed boot components
    components = ["boot", "vbmeta", "recovery"]
    extracted = extract_and_decompress(ap_path, components, work_dir)
    if len(extracted) < 3:
        error("Could not extract all three required boot components.")
        return 1

    flash_cmd = ["samloader", "flash"]
    for comp in ["BOOT", "VBMETA", "RECOVERY"]:
        flash_cmd.extend(["-p", comp, str(extracted[comp])])

    info("Flashing signed stock boot components to clear LOKE Emergency Lock...")
    ret = DiagnosticInterpreter.wrap_command(flash_cmd, "LOKE Emergency Clear Flash")
    if ret == 0:
        print(f"\n{Style.GREEN}{Style.BOLD}🎉 Success! The LOKE Emergency Recovery lock has been cleared.{Style.RESET}")
        print("\nNext Steps:")
        print("1. The device will reboot automatically.")
        print("2. It will drop into Stock Recovery or bootloop because the 'super' partition still holds custom ROM data.")
        print("3. Choose your path forward:")
        print(f"   • {Style.BOLD}Path A (Restore Stock One UI):{Style.RESET} Run 'exynos-rescue raw-stock' to flash raw SUPER & DTBO.")
        print(f"   • {Style.BOLD}Path B (Re-flash LineageOS):{Style.RESET} Select 'Wipe data/factory reset', 'Reboot to bootloader', and flash custom recovery.")
    return ret


# --- Feature 6: Mode 2 - Full Raw Stock Restore ---
def cmd_raw_stock(args):
    """Mode 2: Flash raw dynamic partitions (SUPER, DTBO) and boot components partition-by-partition,
    completely bypassing the dynamic partition pre-flight metadata assertion crash."""
    banner()
    if not check_prerequisites(["samloader", "tar", "lz4"]):
        return 1

    ap_path = find_ap_tarball(args.ap)
    if not ap_path:
        error("Could not find stock AP archive (AP_*.tar.md5).")
        return 1

    info(f"Using AP archive: {ap_path}")
    work_dir = Path(args.work_dir or "./extracted_stock").resolve()

    warn("Full raw stock restore extracts super.img (~4.5 GB). This will take 1-3 minutes.")
    components = ["super", "dtbo"]
    if args.include_boot:
        components.extend(["boot", "vbmeta", "recovery"])

    extracted = extract_and_decompress(ap_path, components, work_dir)
    if "SUPER" not in extracted or "DTBO" not in extracted:
        error("Failed to extract essential SUPER and DTBO images.")
        return 1

    flash_cmd = ["samloader", "flash"]
    for comp, path in extracted.items():
        flash_cmd.extend(["-p", comp, str(path)])

    info("Flashing raw partitions partition-by-partition to bypass dynamic partition calculation crash...")
    ret = DiagnosticInterpreter.wrap_command(flash_cmd, "Raw Stock OS Flash")
    if ret == 0:
        print(f"\n{Style.GREEN}{Style.BOLD}🎉 Full Raw Stock Flashing Completed!{Style.RESET}")
        print("\nPost-Flash Procedure:")
        print("1. Phone will reboot into Samsung Stock Recovery (data mismatch with One UI).")
        print("2. In Stock Recovery:")
        print("   • Select: 'Wipe data/factory reset' -> Confirm")
        print("   • Select: 'Reboot system now'")
        print("3. Allow 5–10 minutes for One UI first-time boot.")
        print("4. Complete setup, enable Developer Options, and verify 'OEM Unlocking' is ON.")
    return ret


# --- Feature 7: Interactive Diagnostic Guide ---
def cmd_diagnose(args):
    banner()
    print(f"{Style.BOLD}Common Samsung Exynos / LOKE Error Diagnostics & Fixes:{Style.RESET}\n")

    issues = [
        (
            "1. LOKE FAIL! [code: -1] / VaultKeeper Lock",
            "Occurs when custom/unsigned images (TWRP, Lineage Recovery) are flashed while the bootloader is in Emergency state or KG state is Prenormal.",
            "Run 'exynos-rescue rescue-boot' with stock AP to clear emergency flag, or 'exynos-rescue raw-stock' to boot One UI and verify OEM Unlocking."
        ),
        (
            "2. Heimdall / Odin Bulk Read Timeout (error: -7)",
            "Older Exynos bootloaders reject Odin v4 packets, and Heimdall fails to send session-end packets on modern chips (e.g. Exynos 850).",
            "Use samloader-rs or ExynosRescue, which uses the compliant Odin v3/v4 session sequence."
        ),
        (
            "3. samloader-rs Dynamic Partition Pre-Flight Crash",
            "Occurs when flashing full AP_*.tar.md5 after LineageOS because samloader fails to parse the scrambled dynamic partition metadata inside 'super'.",
            "Run 'exynos-rescue raw-stock' to extract raw 'super.img' and 'dtbo.img' and flash via '-p' directly."
        ),
        (
            "4. Post-Rescue Bootloop",
            "Stock boot components (boot/recovery/vbmeta) are successfully flashed, but the 'super' partition still contains custom ROM blocks.",
            "Run 'exynos-rescue raw-stock' to flash stock One UI, or wipe in stock recovery and re-enter bootloader for custom ROM."
        ),
        (
            "5. USB Endpoint Hang / Replug Required",
            "An aborted LOKE session leaves the USB controller in an unresponsive state.",
            "Unplug USB, run 'sudo exynos-rescue fix-usb', wait 5 seconds, and plug back in."
        )
    ]

    for title, explanation, solution in issues:
        print(f"{Style.CYAN}{Style.BOLD}{title}{Style.RESET}")
        print(f"  {Style.YELLOW}Cause:{Style.RESET} {explanation}")
        print(f"  {Style.GREEN}Fix:{Style.RESET}   {solution}\n")
    return 0


# --- Main Entry Point ---
def main():
    parser = argparse.ArgumentParser(
        prog="exynos-rescue",
        description="Zero-friction Linux recovery orchestrator for Samsung Exynos devices."
    )
    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # fix-usb
    p_usb = subparsers.add_parser("fix-usb", help="Sanitize USB permissions, stop ModemManager, and unload cdc_acm")
    p_usb.set_defaults(func=cmd_fix_usb)

    # detect
    p_det = subparsers.add_parser("detect", help="Detect connected device and parse model, region, and state")
    p_det.set_defaults(func=cmd_detect)

    # fetch
    p_fetch = subparsers.add_parser("fetch", help="Check and download official stock firmware")
    p_fetch.add_argument("-m", "--model", help="Device model (e.g. SM-A217F)")
    p_fetch.add_argument("-r", "--region", help="Sales Code / CSC (e.g. INS)")
    p_fetch.add_argument("-o", "--out-dir", default="./firmware", help="Directory to save firmware")
    p_fetch.add_argument("--unzip", action="store_true", help="Automatically unzip firmware archive")
    p_fetch.set_defaults(func=cmd_fetch)

    # rescue-boot
    p_boot = subparsers.add_parser("rescue-boot", help="Mode 1: Clear LOKE Emergency Recovery lock with signed boot components")
    p_boot.add_argument("--ap", help="Path to stock AP_*.tar.md5 archive")
    p_boot.add_argument("--work-dir", help="Directory for temporary extracted images")
    p_boot.set_defaults(func=cmd_rescue_boot)

    # raw-stock
    p_raw = subparsers.add_parser("raw-stock", help="Mode 2: Flash raw SUPER & DTBO partitions to bypass pre-flight crash")
    p_raw.add_argument("--ap", help="Path to stock AP_*.tar.md5 archive")
    p_raw.add_argument("--work-dir", help="Directory for temporary extracted images")
    p_raw.add_argument("--include-boot", action="store_true", default=True, help="Include boot, recovery, and vbmeta in the flash")
    p_raw.set_defaults(func=cmd_raw_stock)

    # diagnose
    p_diag = subparsers.add_parser("diagnose", help="Show explanation and fixes for common Exynos flashing errors")
    p_diag.set_defaults(func=cmd_diagnose)

    if len(sys.argv) == 1:
        parser.print_help()
        sys.exit(0)

    args = parser.parse_args()
    if hasattr(args, "func"):
        sys.exit(args.func(args) or 0)
    else:
        parser.print_help()
        sys.exit(1)

if __name__ == "__main__":
    main()
