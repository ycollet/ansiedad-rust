# CI and release procedure

[CI](workflows/ci.yml) runs on branch pushes, pull requests, and manual dispatch.
It checks Rust formatting, Clippy, release metadata tests, and builds and tests
both the default terminal-only configuration and the `gui` configuration on
Linux, macOS, and Windows. GUI command-line tests run without a desktop;
interactive rendering and clipboard access still need manual testing.

## Release installers

The [Release installers workflow](workflows/release.yml) builds both `ansiedad`
and `ansiedad-gui`, includes the license, README, and example patches, and verifies
the installed applications using their display-independent `--check` commands.

| Platform | Installer | Architecture |
| --- | --- | --- |
| Debian 12+ / compatible Ubuntu releases | `.deb` | x86-64 |
| Fedora 43+ | `.rpm` | x86-64 |
| macOS 11+ | `.dmg` | Apple Silicon and Intel, separate images |
| Windows 10+ | `.exe` | x86-64 |

### 1. Update the version

From the commit intended for release, update `[package].version` in `Cargo.toml`,
then refresh `Cargo.lock` and check both applications:

```sh
cargo check --all-features
cargo test --locked --all-features
python3 -m unittest discover -s scripts -p 'test_*.py'
git diff -- Cargo.toml Cargo.lock
git add Cargo.toml Cargo.lock
git commit -m "Release 0.2.0"
git push origin HEAD
```

Use the actual version in the commit message. Merge any required pull request
and check out the resulting release commit before tagging.

### 2. Create and push a version tag

The tag must be `vVERSION` or `VERSION` and match `Cargo.toml` exactly after the
optional `v` is removed. For example, with package version `0.2.0`:

```sh
GITHUB_REF_NAME=v0.2.0 python3 scripts/release_metadata.py
git tag -a v0.2.0 -m "ANSIedad 0.2.0"
git push origin v0.2.0
```

Prereleases such as `0.2.0-rc.1` are supported and marked as GitHub prereleases.
SemVer build metadata (`+build.1`) is not supported. Each numeric version
component must be at most 65535 for Windows installer compatibility.

### 3. Monitor the release

Open **Actions > Release installers**. Only after every platform succeeds does
the publishing job upload all installers and `SHA256SUMS` and publish the GitHub
Release with automatically generated notes. Matching source archives are
available on the same release page. Installers are also workflow artifacts.

No signing secrets are needed: Windows installers are unsigned, and macOS apps
are ad-hoc signed, not Developer ID-signed or notarized. Operating systems may
show security warnings.

On macOS, drag `ANSIedad.app` into Applications. Finder launches the GUI; run
the terminal application with:

```sh
/Applications/ANSIedad.app/Contents/MacOS/ansiedad
```

On Windows, Start menu entries launch either application. To use command-line
arguments, run the executables from the installation directory (by default
`%LOCALAPPDATA%\Programs\ANSIedad`); the installer does not change `PATH`.
Linux packages install both executables in `/usr/bin` and add a GUI desktop entry.

## Manual builds and failures

Use **Actions > Release installers > Run workflow** to build installers from
the selected branch or tag without publishing a release. Manual runs use that
commit's Cargo version, even when a tag is selected. Download results from the
run's **Artifacts** section. The workflow must be on the default branch before
the manual dispatch button appears.

Rerun failed jobs for transient failures. Publishing reruns replace assets for
the same tag; failures may leave a draft release. If source or workflow changes
are necessary, commit the fix and release a new version instead of moving a tag.
