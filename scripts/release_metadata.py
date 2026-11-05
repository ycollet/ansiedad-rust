"""Validate a tag or manually built package version for native installers."""

import os
from pathlib import Path
import re
import tomllib


def metadata(tag, package_version):
    version = tag.removeprefix("v")
    match = re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
        r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?",
        version,
    )
    if not match:
        raise ValueError("Release tags must be VERSION or vVERSION (SemVer without build metadata).")
    if version != package_version:
        raise ValueError(
            f"Tag version {version} does not match Cargo.toml version {package_version}."
        )
    if any(int(match[index]) > 65535 for index in (1, 2, 3)):
        raise ValueError("Windows installer version components cannot exceed 65535.")
    prerelease = match[4]
    if prerelease and any(
        part.isdigit() and len(part) > 1 and part.startswith("0")
        for part in prerelease.split(".")
    ):
        raise ValueError("Numeric prerelease identifiers cannot have leading zeroes.")
    base = ".".join(match.group(1, 2, 3))
    return {
        "version": version,
        "base_version": base,
        "debian_version": base + ("~" + prerelease if prerelease else ""),
        "rpm_release": "0." + prerelease.replace("-", ".") if prerelease else "1",
        "prerelease": str(bool(prerelease)).lower(),
    }


if __name__ == "__main__":
    package = tomllib.loads(Path("Cargo.toml").read_text())["package"]
    try:
        tag = (
            package["version"]
            if os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
            else os.environ["GITHUB_REF_NAME"]
        )
        values = metadata(tag, package["version"])
    except (KeyError, ValueError) as error:
        raise SystemExit(str(error)) from error
    for key, value in values.items():
        print(f"{key}={value}")
