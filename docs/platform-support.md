# Install the binary for your platform

The stable release `v0.25.0` targets Linux x86_64 and macOS on
Apple Silicon and Intel. The public release workflow is the authoritative path
for validating native builds, packaged installation, and public asset
installation; this source snapshot does not claim those checks have passed for
this release. WSL uses the same Linux artifact inside Ubuntu. See the
[public release workflow](https://github.com/envy404/exitbind/actions/workflows/release.yml).

Use the pinned installer in the [README](../README.md#see-a-false-done-refused).
It selects an archive from the operating system and architecture, checks its
SHA-256, and installs one executable. No Rust, Python, Node.js, or model account
is needed to run the installed binary or `exitbind benchmark`.

| Installed target | Release archive target | Validation boundary |
| --- | --- | --- |
| Linux x86_64 / amd64 | `x86_64-unknown-linux-gnu` | Required release gates: native build, packaged installation, and public asset installation. |
| Ubuntu on WSL 2 | `x86_64-unknown-linux-gnu` | Exercises the same Linux artifact inside Ubuntu; keep project, agent host, and Exitbind in the same Ubuntu distribution. |
| macOS on Apple Silicon | `aarch64-apple-darwin` | Required release gates: native macOS 15 build, packaged installation, and public asset installation. |
| macOS on Intel | `x86_64-apple-darwin` | Required release gates: native macOS 15 build, packaged installation, and public asset installation. |

The [CI](../.github/workflows/ci.yml) and
[release workflow](../.github/workflows/release.yml) require both native Mac
architectures. Older macOS versions have not been validated by this matrix.
Cross-compilation alone is not installation evidence.

The public release workflow rechecks transferred archive checksums and
provenance, then matches each separately published executable to the executable
inside its archive. These checks are not Apple Developer ID signing or
notarization.

Native Windows and other Linux architectures are unsupported. The installer
rejects unsupported operating-system/architecture pairs before fetching an
archive. The ledger's Unix no-follow file-opening requirement is retained;
[WSL 2](windows-wsl.md) is the supported Windows route. Building from source
does not turn an untested platform into supported installation.

After installation, run `exitbind version` and `exitbind benchmark` before
initializing your project. A checksum or platform error is a failed install;
do not bypass it. See [updates and removal](../REFERENCE.md#removal) for the
explicit managed-skill refresh and the records left by uninstalling.
