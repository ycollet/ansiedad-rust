from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import runpy
import unittest
from unittest.mock import patch

from release_metadata import metadata


class ReleaseMetadataTests(unittest.TestCase):
    def test_stable_and_prerelease_package_versions(self):
        for tag in ["v1.2.3", "1.2.3"]:
            self.assertEqual(metadata(tag, "1.2.3"), {
                "version": "1.2.3",
                "base_version": "1.2.3",
                "debian_version": "1.2.3",
                "rpm_release": "1",
                "prerelease": "false",
            })
        self.assertEqual(metadata("v1.2.3-rc.1", "1.2.3-rc.1"), {
            "version": "1.2.3-rc.1",
            "base_version": "1.2.3",
            "debian_version": "1.2.3~rc.1",
            "rpm_release": "0.rc.1",
            "prerelease": "true",
        })
        self.assertEqual(
            metadata("v1.2.3-beta-1", "1.2.3-beta-1")["rpm_release"], "0.beta.1"
        )

    def test_version_mismatch_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            metadata("v1.2.3", "0.1.0")

    def test_workflow_events_select_the_expected_version(self):
        script = Path(__file__).with_name("release_metadata.py")
        for event, ref in [
            ("workflow_dispatch", "main"),
            ("workflow_dispatch", "v9.9.9"),
            ("push", "v1.2.3"),
        ]:
            with self.subTest(event=event, ref=ref):
                output = io.StringIO()
                with (
                    patch.dict(os.environ, {
                        "GITHUB_EVENT_NAME": event,
                        "GITHUB_REF_NAME": ref,
                    }, clear=True),
                    patch.object(Path, "read_text", return_value='[package]\nversion = "1.2.3"\n'),
                    redirect_stdout(output),
                ):
                    runpy.run_path(str(script), run_name="__main__")
                self.assertEqual(
                    dict(line.split("=", 1) for line in output.getvalue().splitlines()),
                    metadata("v1.2.3", "1.2.3"),
                )

    def test_invalid_tags_cannot_become_package_metadata(self):
        for version in [
            "latest", "1.2", "01.2.3", "1.2.3+build", "1.2.3-01",
            "1.2.3-rc..1", "1.2.3\ninjected=value", "1.2.3;echo bad",
            "65536.0.0", "0.65536.0", "0.0.65536",
        ]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                metadata(version, version)


if __name__ == "__main__":
    unittest.main()
