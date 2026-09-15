"""Validate a release tag against Cargo and extract its dated changelog section."""

import datetime
from pathlib import Path
import re
import sys
import tomllib


def release_notes(manifest: str, changelog: str, tag: str = "") -> tuple[str, str]:
    version = tomllib.loads(manifest)["workspace"]["package"]["version"]
    tag = tag or f"v{version}"
    number = r"(?:0|[1-9][0-9]*)"
    if not re.fullmatch(rf"v{number}\.{number}\.{number}(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?", tag):
        raise ValueError("Expected vMAJOR.MINOR.PATCH or a SemVer prerelease tag")
    if "-" in tag:
        for part in tag.split("-", 1)[1].split("."):
            if part.isdigit() and len(part) > 1 and part.startswith("0"):
                raise ValueError("Numeric prerelease identifiers cannot have leading zeros")
    if tag != f"v{version}":
        raise ValueError(f"Tag {tag} does not match workspace version {version}")
    sections = re.split(r"(?m)^## ", changelog)
    matches = [s for s in sections[1:] if s.startswith(f"[{version}]")]
    if len(matches) != 1:
        raise ValueError(f"Expected exactly one changelog section for {version}")
    heading, _, body = matches[0].partition("\n")
    dated = re.fullmatch(rf"\[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})", heading)
    if not dated:
        raise ValueError("Release changelog heading must include YYYY-MM-DD")
    datetime.date.fromisoformat(dated[1])
    references = re.findall(r"(?m)^\[[^\]]+\]:\s+\S+.*$", changelog)
    body = re.split(r"(?m)^\[[^\]]+\]:", body)[0].strip()
    if not body:
        raise ValueError("Release changelog section cannot be empty")
    return tag, body + "\n" + ("\n" + "\n".join(references) + "\n" if references else "")


if __name__ == "__main__":
    tag, notes = release_notes(
        Path("Cargo.toml").read_text(encoding="utf-8"),
        Path("CHANGELOG.md").read_text(encoding="utf-8"),
        sys.argv[1] if len(sys.argv) > 1 else "",
    )
    Path("target").mkdir(exist_ok=True)
    Path("target/release-notes.md").write_text(notes, encoding="utf-8")
    print(tag)
