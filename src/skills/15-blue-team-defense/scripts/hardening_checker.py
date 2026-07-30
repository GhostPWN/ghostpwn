#!/usr/bin/env python3
"""
System Hardening Checker
Validates system configuration against security best practices.

Repository: https://github.com/Masriyan/Claude-Code-CyberSecurity-Skill
"""

import argparse
import json
import logging
import os
import platform
import re
import shutil
import subprocess
import time
from typing import Any, Dict, List

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger(__name__)


def firewall_active(command: str, output: str) -> bool:
    output = output.lower()
    if command == "ufw":
        return "status: active" in output
    if command == "iptables":
        input_chain = re.search(
            r"chain input\b.*?(?=\nchain |\Z)", output, re.DOTALL
        )
        return bool(input_chain and re.search(
            r"policy (?!accept)|^\s*(?:drop|reject)\b",
            input_chain.group(0),
            re.MULTILINE,
        ))
    return any(
        "hook input" in chain
        and re.search(r"policy (?:drop|reject)", chain)
        for chain in re.findall(r"chain\b.*?\{.*?\}", output, re.DOTALL)
    )


class LinuxHardeningChecker:
    """Check Linux system hardening against CIS-style benchmarks."""

    def __init__(self):
        self.findings: List[Dict] = []

    def check_ssh_config(self) -> None:
        try:
            result = subprocess.run(
                ["sshd", "-T"], capture_output=True, text=True, timeout=5
            )
        except (FileNotFoundError, PermissionError, subprocess.TimeoutExpired):
            self.findings.append({"id": "SSH-000", "severity": "INFO",
                                  "title": "Effective SSH config unavailable", "status": "SKIP"})
            return
        if result.returncode != 0:
            self.findings.append({"id": "SSH-000", "severity": "INFO",
                                  "title": "Effective SSH config unavailable", "status": "SKIP"})
            return

        config = {}
        for line in result.stdout.splitlines():
            key, _, value = line.partition(" ")
            if value:
                config[key.lower()] = value.strip().lower()

        def int_at_most(key: str, maximum: int) -> bool:
            try:
                return 0 < int(config.get(key, "0")) <= maximum
            except ValueError:
                return False

        checks = [
            ("SSH-001", "HIGH", "Root login disabled", config.get("permitrootlogin") == "no"),
            ("SSH-002", "HIGH", "Password auth disabled", config.get("passwordauthentication") == "no"),
            ("SSH-003", "MEDIUM", "X11 forwarding disabled", config.get("x11forwarding") == "no"),
            ("SSH-004", "MEDIUM", "Max auth tries limited", int_at_most("maxauthtries", 4)),
            ("SSH-006", "LOW", "Login grace time limited", int_at_most("logingracetime", 60)),
            ("SSH-007", "MEDIUM", "Strong ciphers only (no CBC/arcfour)",
             bool(config.get("ciphers")) and not re.search(r"cbc|arcfour|3des", config["ciphers"])),
            ("SSH-008", "MEDIUM", "Modern KEX algorithms enabled",
             bool(re.search(r"curve25519|sntrup|ecdh-sha2", config.get("kexalgorithms", "")))),
            ("SSH-009", "LOW", "ClientAlive idle timeout set",
             config.get("clientaliveinterval", "0").isdigit()
             and int(config["clientaliveinterval"]) > 0),
        ]
        for cid, sev, title, passed in checks:
            status = "PASS" if passed else "FAIL"
            self.findings.append({"id": cid, "severity": sev, "title": title, "status": status})

    def check_firewall(self) -> None:
        for fw_cmd in ["ufw status", "iptables -L -n", "nft list ruleset"]:
            cmd = fw_cmd.split()[0]
            if shutil.which(cmd):
                try:
                    result = subprocess.run(fw_cmd.split(), capture_output=True, text=True, timeout=5)
                    if result.returncode != 0:
                        continue
                    active = firewall_active(cmd, result.stdout)
                    self.findings.append({"id": "FW-001", "severity": "HIGH", "title": f"Firewall active ({cmd})", "status": "PASS" if active else "FAIL"})
                    return
                except (subprocess.TimeoutExpired, FileNotFoundError, PermissionError):
                    pass
        self.findings.append({"id": "FW-001", "severity": "HIGH", "title": "Firewall status", "status": "SKIP"})

    def check_filesystem(self) -> None:
        suid_count = 0
        for root, dirs, files in os.walk("/usr"):
            for f in files:
                path = os.path.join(root, f)
                try:
                    if os.path.isfile(path) and os.stat(path).st_mode & 0o4000:
                        suid_count += 1
                except (OSError, PermissionError):
                    pass
            if suid_count > 50:
                break
        self.findings.append({"id": "FS-001", "severity": "MEDIUM", "title": f"SUID binaries count: {suid_count}",
                             "status": "PASS" if suid_count < 30 else "WARN"})
        world_writable = os.path.exists("/tmp") and os.stat("/tmp").st_mode & 0o0002
        self.findings.append({"id": "FS-002", "severity": "LOW", "title": "/tmp world-writable check",
                             "status": "INFO" if world_writable else "PASS"})

    def check_services(self) -> None:
        unnecessary = ["telnet", "rsh", "rlogin", "tftp", "vsftpd"]
        for svc in unnecessary:
            try:
                result = subprocess.run(
                    ["systemctl", "show", svc, "--property=LoadState,ActiveState"],
                    capture_output=True, text=True, timeout=3
                )
                state = dict(
                    line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
                )
                if result.returncode != 0 or not state:
                    status = "SKIP"
                else:
                    status = "FAIL" if state.get("ActiveState") == "active" else "PASS"
                self.findings.append({"id": f"SVC-{svc}", "severity": "MEDIUM",
                                     "title": f"Unnecessary service: {svc}", "status": status})
            except (FileNotFoundError, subprocess.TimeoutExpired):
                self.findings.append({"id": f"SVC-{svc}", "severity": "MEDIUM",
                                     "title": f"Unnecessary service: {svc}", "status": "SKIP"})

    def check_audit(self) -> None:
        try:
            result = subprocess.run(["systemctl", "is-active", "auditd"], capture_output=True, text=True, timeout=3)
            state = result.stdout.strip()
            status = "PASS" if state == "active" else ("FAIL" if state in ("inactive", "failed") else "SKIP")
        except (FileNotFoundError, subprocess.TimeoutExpired):
            status = "SKIP"
        self.findings.append({"id": "AUD-001", "severity": "HIGH", "title": "Audit daemon (auditd) running",
                             "status": status})

    def check_sysctl(self) -> None:
        """Validate kernel network/exec hardening sysctls (CIS-aligned)."""
        expected = {
            "SYS-001": ("HIGH", "ASLR fully enabled", "kernel.randomize_va_space", "2"),
            "SYS-002": ("MEDIUM", "IP forwarding disabled", "net.ipv4.ip_forward", "0"),
            "SYS-003": ("MEDIUM", "Reverse-path filtering on", "net.ipv4.conf.all.rp_filter", "1"),
            "SYS-004": ("MEDIUM", "ICMP redirects not accepted", "net.ipv4.conf.all.accept_redirects", "0"),
            "SYS-005": ("MEDIUM", "Source routing disabled", "net.ipv4.conf.all.accept_source_route", "0"),
            "SYS-006": ("LOW", "TCP SYN cookies enabled", "net.ipv4.tcp_syncookies", "1"),
            "SYS-007": ("MEDIUM", "ptrace scope restricted", "kernel.yama.ptrace_scope", "1"),
        }
        for cid, (sev, title, key, want) in expected.items():
            try:
                result = subprocess.run(["sysctl", "-n", key], capture_output=True, text=True, timeout=3)
                val = result.stdout.strip()
                status = "PASS" if val == want else ("FAIL" if val else "SKIP")
            except (FileNotFoundError, subprocess.TimeoutExpired):
                status = "SKIP"
            self.findings.append({"id": cid, "severity": sev, "title": title, "status": status})

    def check_kernel_modules(self) -> None:
        """Flag risky/legacy filesystem & network modules that should be disabled."""
        risky = ["cramfs", "freevxfs", "hfs", "hfsplus", "squashfs", "udf", "usb-storage", "dccp", "sctp"]
        try:
            result = subprocess.run(["lsmod"], capture_output=True, text=True, timeout=3)
        except (FileNotFoundError, subprocess.TimeoutExpired):
            result = None
        if result is None or result.returncode != 0:
            self.findings.append({"id": "MOD-000", "severity": "INFO",
                                  "title": "Loaded kernel modules unavailable", "status": "SKIP"})
            return
        loaded = result.stdout
        for mod in risky:
            present = bool(re.search(rf"^{re.escape(mod)}\b", loaded, re.MULTILINE))
            self.findings.append({"id": f"MOD-{mod}", "severity": "LOW",
                                  "title": f"Legacy/risky module not loaded: {mod}",
                                  "status": "FAIL" if present else "PASS"})

    REMEDIATION = {
        "SSH-001": "Set 'PermitRootLogin no' in /etc/ssh/sshd_config and reload sshd.",
        "SSH-002": "Set 'PasswordAuthentication no' and use key/cert auth.",
        "SSH-007": "Set 'Ciphers' to AEAD/CTR only (e.g. chacha20-poly1305, aes256-gcm).",
        "SSH-008": "Set 'KexAlgorithms' to curve25519-sha256 (or sntrup761x25519 for PQC hybrid).",
        "FW-001": "Enable a host firewall (ufw enable / nft default-deny inbound).",
        "AUD-001": "Install and enable auditd; deploy a CIS/Neo23x0 audit ruleset.",
        "SYS-001": "echo 'kernel.randomize_va_space=2' >> /etc/sysctl.d/99-hardening.conf; sysctl --system",
        "SYS-007": "Set kernel.yama.ptrace_scope=1 to restrict process tracing.",
    }

    def _annotate_remediation(self) -> None:
        for f in self.findings:
            if f["status"] in ("FAIL", "WARN"):
                hint = self.REMEDIATION.get(f["id"])
                if not hint and f["id"].startswith("MOD-"):
                    mod = f["id"].split("-", 1)[1]
                    hint = f"Blacklist module: echo 'install {mod} /bin/true' >> /etc/modprobe.d/hardening.conf"
                if not hint and f["id"].startswith("SVC-"):
                    hint = "Disable & mask the service: systemctl disable --now <svc>; systemctl mask <svc>"
                if hint:
                    f["remediation"] = hint

    def run(self) -> Dict[str, Any]:
        logger.info("=" * 50)
        logger.info("Linux Hardening Check")
        logger.info("=" * 50)
        self.check_ssh_config()
        self.check_firewall()
        self.check_filesystem()
        self.check_services()
        self.check_audit()
        self.check_sysctl()
        self.check_kernel_modules()
        self._annotate_remediation()
        passed = sum(1 for f in self.findings if f["status"] == "PASS")
        failed = sum(1 for f in self.findings if f["status"] == "FAIL")
        return {
            "os": "Linux", "hostname": platform.node(),
            "total_checks": len(self.findings), "passed": passed, "failed": failed,
            "score_pct": round(passed / max(len(self.findings), 1) * 100, 1),
            "findings": self.findings,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }


def main():
    parser = argparse.ArgumentParser(description="System Hardening Checker",
                                     epilog="https://github.com/Masriyan/Claude-Code-CyberSecurity-Skill")
    parser.add_argument("--os", choices=["ubuntu", "centos", "windows", "auto"], default="auto")
    parser.add_argument("--output", "-o", help="Output file (JSON)")
    parser.add_argument("--verbose", "-v", action="store_true")
    args = parser.parse_args()
    if args.verbose:
        logging.getLogger().setLevel(logging.DEBUG)

    checker = LinuxHardeningChecker()
    results = checker.run()

    if args.output:
        with open(args.output, "w") as f:
            json.dump(results, f, indent=2)
        logger.info("Report saved to %s", args.output)
    else:
        print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
