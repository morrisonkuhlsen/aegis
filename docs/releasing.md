# Releasing MK aeGIS

Pushing a version tag runs [`.github/workflows/release.yml`](../.github/workflows/release.yml).
It builds the desktop app and the CLI (which also serves MCP) for Linux, Windows and macOS and
publishes them as a GitHub release, with a checksum for every file.

## Cutting a release

1. **Bump the version.** It lives in one place, `[workspace.package] version` in the root
   `Cargo.toml`, and every crate inherits it:

   ```sh
   cargo xtask version                 # prints the current version, e.g. 0.1.1
   cargo xtask version set 0.2.0       # or 0.2.0-rc.1; updates Cargo.toml and Cargo.lock
   ```

   It refuses anything a tag can't name (`v0.2.0`, `0.2`, `0.2.0+build`). Merge the two files
   through a pull request once CI passes.
2. **Tag the merge commit and push the tag:**

   ```sh
   git switch main && git pull
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. **Wait for the workflow** (Actions › Release, about 10 minutes). It runs three jobs:
   - `version` checks that the tag is `v` + the workspace version. A mismatch stops the run
     before anything is built.
   - `build` builds each platform, checks that `mkaegis --version` and `mkaegis-cli --version`
     report the version, and packages it.
   - `publish` runs only when every build passed. It creates the release `MK aeGIS v0.2.0`, with
     notes generated from the pull requests merged since the previous tag, the archives and
     `SHA256SUMS.txt`.
4. **Check it.** Download the archive for your system, run `mkaegis-cli --version`, open
   `mkaegis --sample` and run
   `mkaegis-cli run --sample --cmd 'analysis.summaryStatistics={"layer":"Cidades","field":"POP"}'`.
   Then edit the release on GitHub to put the highlights above the generated notes.

A tag with a suffix (`v0.2.0-rc.1`, from version `0.2.0-rc.1`) is published as a
**pre-release**, which the Releases page never shows as the latest.

## What a release contains

| File                                   | Contents                                       |
|----------------------------------------|------------------------------------------------|
| `mkaegis-<ver>-linux-x86_64.tar.gz`    | `mkaegis`, `mkaegis-cli`                       |
| `mkaegis-<ver>-windows-x64.zip`        | `mkaegis.exe`, `mkaegis-cli.exe`               |
| `mkaegis-<ver>-macos-universal.tar.gz` | the same two, each for Apple silicon and Intel |
| `SHA256SUMS.txt`                       | the SHA-256 checksum of every archive          |

Every archive also holds `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` (the licences,
and the brand assets the app embeds, which they don't cover) and `ATTRIBUTION.md`.
The macOS binaries are universal: `lipo` joins the `aarch64-apple-darwin` and
`x86_64-apple-darwin` builds. The Linux build runs on Ubuntu 24.04, so it needs glibc 2.39 or
later.

## Dry runs

A pull request that changes `release.yml` runs the same builds without publishing anything, and
so does **Actions › Release › Run workflow** on any branch. The archives are on the run's page
under *Artifacts*.

## When something goes wrong

- **The tag doesn't match the version.** Delete the tag (`git push origin :refs/tags/v0.2.0`,
  then `git tag -d v0.2.0`), fix `Cargo.toml` through a pull request and tag again.
- **A build failed.** Nothing was published. For a transient failure (network, runner), use
  *Re-run failed jobs*. For a real one, fix it on `main`, delete the tag and tag the fixing
  commit with the same version.
- **A published release is wrong.** Don't reuse its version, because someone may have
  downloaded it already. Delete it (`gh release delete v0.2.0 --cleanup-tag`) or mark it as a
  pre-release, and release the next patch version.

## Not yet

- **Code signing.** The binaries are unsigned, so Windows SmartScreen and macOS Gatekeeper ask
  for confirmation on first launch (see *Downloads* in the README). Signing needs an Apple
  Developer ID and a Windows code-signing certificate, both paid.
- **Installers:** `.msi`, `.dmg`, `.deb`/`.rpm`, AppImage, Flatpak.
- **More platforms:** Linux aarch64, Windows arm64.
- **The web app** ([#38](https://github.com/morrisonkuhlsen/mkaegis/issues/38)).
