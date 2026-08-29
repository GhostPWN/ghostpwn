#!/usr/bin/env python3
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest
import zipfile
from unittest import mock


ROOT = pathlib.Path(__file__).parents[1]


def load(name, relative_path):
    path = ROOT / relative_path
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cvss = load(
    "cvss_calculator",
    "skills/02-vulnerability-scanner/scripts/cvss_calculator.py",
)
deps = load(
    "dependency_auditor",
    "skills/02-vulnerability-scanner/scripts/dependency_auditor.py",
)
models = load(
    "model_supply_chain",
    "skills/16-ai-llm-security/scripts/model_supply_chain.py",
)
hardening = load(
    "hardening_checker",
    "skills/15-blue-team-defense/scripts/hardening_checker.py",
)
apk = load(
    "apk_analyzer",
    "skills/17-mobile-security/scripts/apk_analyzer.py",
)
api_security = load(
    "api_security_tester",
    "skills/09-web-security/scripts/api_security_tester.py",
)


class SecurityScriptChecks(unittest.TestCase):
    def test_api_security_tester_verifies_tls_and_sends_bearer_token(self):
        tester = api_security.APISecurityTester(
            "https://api.example.com", token="secret-token"
        )

        with mock.patch.object(api_security.requests, "request") as request:
            tester._request("GET", "/users")

        request.assert_called_once_with(
            "GET",
            "https://api.example.com/users",
            headers={
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Authorization": "Bearer secret-token",
            },
            json=None,
            params=None,
            timeout=10,
            verify=True,
            allow_redirects=False,
        )

    def test_api_security_tester_allows_explicit_insecure_requests(self):
        tester = api_security.APISecurityTester(
            "https://api.example.com", verify_tls=False
        )

        with mock.patch.object(api_security.requests, "request") as request:
            tester._request("GET", "/health")

        self.assertFalse(request.call_args.kwargs["verify"])

    def test_api_security_tester_cli_warns_for_insecure_mode(self):
        results = {
            "target": "https://api.example.com",
            "total_findings": 0,
            "by_severity": {},
            "findings": [],
        }
        with (
            mock.patch.object(
                sys,
                "argv",
                [
                    "api_security_tester.py",
                    "--base-url",
                    "https://api.example.com",
                    "--token",
                    "secret-token",
                    "--insecure",
                ],
            ),
            mock.patch.object(api_security, "APISecurityTester") as tester_class,
            mock.patch.object(api_security, "print_summary"),
            self.assertLogs(api_security.logger, level="WARNING") as logs,
        ):
            tester_class.return_value.run.return_value = results
            api_security.main()

        tester_class.assert_called_once_with(
            "https://api.example.com",
            None,
            "secret-token",
            10,
            verify_tls=False,
        )
        self.assertIn("TLS certificate verification is disabled", logs.output[0])

    def test_cvss_rounds_up(self):
        self.assertEqual(cvss.round_up(4.01), 4.1)
        self.assertEqual(cvss.round_up(4.0), 4.0)

    def test_dependency_auditor_parses_locked_npm_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            lock = pathlib.Path(directory) / "package-lock.json"
            lock.write_text(json.dumps({
                "packages": {
                    "": {"name": "app", "version": "1.0.0"},
                    "node_modules/example": {"version": "2.3.4"},
                }
            }))
            packages = deps.DependencyAuditor().parse_package_lock(str(lock))
        self.assertEqual(packages, [{"name": "example", "version": "2.3.4"}])

    def test_dependency_auditor_parses_cvss_vectors(self):
        severity = deps.DependencyAuditor().get_severity({
            "severity": [{
                "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"
            }]
        })
        self.assertEqual(severity, "CRITICAL")

    def test_dependency_auditor_reports_osv_failures(self):
        auditor = deps.DependencyAuditor()
        with mock.patch.object(
            deps.urllib.request, "urlopen", side_effect=OSError("offline")
        ):
            self.assertEqual(auditor.query_osv("example", "1.0.0", "PyPI"), [])
        self.assertEqual(auditor.query_errors, ["example@1.0.0: offline"])

    def test_model_scanner_caps_expanded_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = pathlib.Path(directory) / "model.pt"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("data.pkl", b"x" * 32)
            original_limit = models.MAX_PICKLE_BYTES
            models.MAX_PICKLE_BYTES = 16
            try:
                result = models.scan_file(str(archive))
            finally:
                models.MAX_PICKLE_BYTES = original_limit
        self.assertIn("exceeds", result["findings"][0])

    def test_firewall_requires_an_enforcing_configuration(self):
        self.assertFalse(hardening.firewall_active(
            "iptables", "Chain INPUT (policy ACCEPT)\ntarget prot opt source destination"
        ))
        self.assertTrue(hardening.firewall_active(
            "iptables", "Chain INPUT (policy DROP)\ntarget prot opt source destination"
        ))
        self.assertFalse(hardening.firewall_active(
            "iptables",
            "Chain INPUT (policy ACCEPT)\ntarget prot opt source destination\n"
            "Chain OUTPUT (policy ACCEPT)\nDROP all -- anywhere anywhere",
        ))

    def test_apk_manifest_read_is_bounded(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = pathlib.Path(directory) / "app.apk"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("AndroidManifest.xml", b"x" * 32)
            original_limit = apk.MAX_ARCHIVE_ENTRY_BYTES
            apk.MAX_ARCHIVE_ENTRY_BYTES = 16
            try:
                manifest = apk.manifest_from_zip(str(archive))
            finally:
                apk.MAX_ARCHIVE_ENTRY_BYTES = original_limit
        self.assertEqual(manifest, "")


if __name__ == "__main__":
    unittest.main()
