# Contributing to MK aeGIS

Thank you for helping. MK aeGIS is a statistical GIS, and people trust it with their data, their
maps and their results, so a few rules are strict. The full set, for people and AI agents alike, is
in [`AGENTS.md`](AGENTS.md); this page is the short version.

## Before you start

- **Pick from the ranked list.** [`docs/gaps.md`](docs/gaps.md) orders the open issues: lost work
  and crashes first, then wrong statistical results, then the statistics, then the rest. Comment on
  the issue to say you're on it.
- **Read the architecture.** [`docs/architecture.md`](docs/architecture.md): the crates in layers,
  and why everything a person can do is a command.
- **New dependency?** Check [`docs/adr/0001-libraries.md`](docs/adr/0001-libraries.md) first:
  permissive, pure Rust, and wasm-clean below the UI.

## The rules that are never bent

- **Clean-room.** No GPL, AGPL or LGPL code (QGIS, GRASS, GeoDa, R's spdep, GEOS, PostGIS…), and
  don't read their sources. Permissively licensed GIS projects aren't ported either. File formats
  and math come from public specifications and the published literature; say in the pull request
  which ones you used.
- **No one else's assets.** No other vendor's or project's icons, images, styles, symbols, colour
  ramps, basemaps, sample data or help text. Every asset file needs a row in
  [`ATTRIBUTION.md`](ATTRIBUTION.md).
- **Never crash.** No `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or `unsafe` outside
  tests. Input from files, commands and coordinates is hostile: check it, cap it, return errors.
  A crash fix comes with a regression test.
- **Statistics come with proof.** A statistic lands with tests against independently computed
  values, and its output says how to judge it (inference method, permutations, p-values,
  multiple-testing correction).
- **Everything is a command.** User-visible behaviour is a command in `crates/engine/src/cmd/`
  with tests; the UI only calls commands.

## Making the change

```sh
cargo run --release -p mkaegis -- --sample           # the app with the sample map
cargo xtask ci                                        # every quality gate; must pass
```

`cargo xtask ci` runs fmt, clippy with warnings as errors, the tests, and the asset, layer, panic,
wasm and parity checks. CI runs the same on every pull request.

For UI work, look at the result: `cargo run -p mkaegis-ui-egui --example snapshot -- --sample
out.png` renders the window offscreen.

## Commits and pull requests

- One issue per commit, named in the subject: `#46: contiguity on large projected coordinates`.
- Branch from `main` and open a pull request; CI must pass before it merges.
- Add a line to `CHANGELOG.md` under `## [Unreleased]` for anything a user would notice. A
  change that gives different numbers or selections for the same data goes under
  **Changed results**, saying what was computed before and what is computed now.
- Update the docs your change affects (`ROADMAP.md`, `docs/gaps.md`, `docs/parity-checklist.md`
  via `cargo xtask parity`) in the same pull request.
- You'll be credited in *Help › About* by your GitHub username; to choose the name shown, see
  [`docs/contributors.md`](docs/contributors.md).

## Reporting

Bugs and feature requests: [GitHub issues](https://github.com/morrisonkuhlsen/mkaegis/issues/new/choose).
Security problems: privately, as [`SECURITY.md`](SECURITY.md) says, never in a public issue.

By contributing, you agree that your contribution is licensed under MIT OR Apache-2.0, as the
rest of the project.
