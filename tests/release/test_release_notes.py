import unittest

from tools.release_notes import release_notes


class ReleaseNotesTests(unittest.TestCase):
    manifest = '[workspace.package]\nversion = "0.2.0"\n'
    changelog = """# Changelog

## [Unreleased]

Do not publish this.

## [0.2.0] - 2026-09-15

### Added

The current release with a [reference].

## [0.1.0] - 2026-09-01

Do not publish the previous release.

[reference]: https://example.com/docs
"""

    def test_only_requested_version_and_link_definitions_are_extracted(self):
        tag, notes = release_notes(self.manifest, self.changelog, "v0.2.0")
        self.assertEqual(tag, "v0.2.0")
        self.assertIn("The current release", notes)
        self.assertIn("[reference]: https://example.com/docs", notes)
        self.assertNotIn("Do not publish", notes)
        self.assertEqual(release_notes(self.manifest, self.changelog)[0], tag)

    def test_rejects_invalid_mismatched_or_incomplete_release(self):
        cases = [
            (self.changelog, "v0.3.0"),
            (self.changelog, "0.2.0"),
            (self.changelog, "v00.2.0"),
            (self.changelog.replace("[0.2.0]", "[0.3.0]"), "v0.2.0"),
            (self.changelog + "\n## [0.2.0] - 2026-09-15\nDuplicate", "v0.2.0"),
            ("## [0.2.0] - 2026-09-15\n\n## [0.1.0]\nOld", "v0.2.0"),
            ("## [0.2.0] - 2026-09-15\n\n[ref]: https://example.com", "v0.2.0"),
            (self.changelog.replace(" - 2026-09-15", ""), "v0.2.0"),
            (self.changelog.replace("2026-09-15", "2026-02-30"), "v0.2.0"),
        ]
        for changelog, tag in cases:
            with self.subTest(tag=tag, changelog=changelog), self.assertRaises(ValueError):
                release_notes(self.manifest, changelog, tag)

    def test_prerelease_and_final_section(self):
        manifest = self.manifest.replace("0.2.0", "0.3.0-rc.1")
        changelog = "## [0.3.0-rc.1] - 2026-09-15\nPreview.\n"
        self.assertEqual(release_notes(manifest, changelog), ("v0.3.0-rc.1", "Preview.\n"))
        with self.assertRaises(ValueError):
            release_notes(manifest.replace("rc.1", "rc.01"), changelog, "v0.3.0-rc.01")
