# Localization parity

*Updated 2026-10-22, after 0.2.0.*

MK aeGIS speaks English and Brazilian Portuguese (#37). The interface language is chosen in
**View ▸ Language** and kept for later runs; the web app starts in the browser's language. This
page tracks what is translated and what is not yet.

## How it works

- Every string the UI shows goes through `i18n::tr` (or `trf`, with `{named}` values), keyed by
  its English text. The Portuguese catalogue is `crates/ui-egui/i18n/pt-BR.txt`, one
  `English = Português` per line; English is the fallback.
- A key with a `|context` tells two meanings of one English word apart: `Project|tool` is the
  verb (Projetar), `Project` the menu (Projeto).
- `cargo xtask i18n`, part of `cargo xtask ci`, fails on a key missing from the catalogue, on an
  entry nothing uses, on a translation that drops a `{value}`, and on a literal shown straight
  through `label`, `button`, `RichText::new`, `on_hover_text` and the like.
- Numbers on screen follow the language: `1,234.5` in English, `1.234,5` in Portuguese
  (tables, charts, statistics, the scale and the pointer's coordinates, where Portuguese writes
  East and West as L and O). Data, files, commands, the CLI, MCP and the control channel keep
  their canonical forms whatever the language.

## Status

| Part of the interface                                   | Portuguese        |
|---------------------------------------------------------|-------------------|
| Ribbon: tabs, groups, buttons, tooltips                 | yes               |
| Contents, attribute table, status bar, view rail        | yes               |
| Geoprocessing forms and their results                   | yes               |
| Symbology, Charts and History panes                     | yes               |
| Dialogs (selection, fields, coordinate system, about)   | yes               |
| File backstage and the splash screen                    | yes               |
| Command labels in search, history and the status bar    | yes               |
| Numbers on screen (decimal comma, thousands point)      | yes               |
| Messages a command returns (`added 23 features`)        | not yet: English  |
| Errors and warnings from the engine and file readers    | not yet: English  |
| The map's own name, layer and field names (data)        | never: data       |

The engine's messages and errors are built inside the commands, which every client shares (the
CLI and MCP among them); translating them needs message keys in the engine, the next step.

## Revision history

| Date       | Change                             |
|------------|------------------------------------|
| 2026-10-22 | First version: Portuguese (Brazil) |
