# Control protocol

*Updated 2026-10-21. Source: `crates/ui-egui/src/control.rs`.*

The control channel lets agents and scripts drive the running desktop app and look at it. Start
the app with a port:

```sh
mkaegis --sample --control 7979
```

It listens on `127.0.0.1` only. There is no authentication: any program on the same machine can
connect, run commands (including ones that write files) and take screenshots. Use it on a machine
you trust, and leave `--control` off otherwise. See [`SECURITY.md`](../SECURITY.md).

## Messages

One JSON object per line, in both directions. A request:

```json
{"id": 1, "method": "engine.execute", "params": {"command": "map.selectByAttributes", "params": {"layer": "Cidades", "where": "POP > 100000"}}}
```

The reply carries the same `id` and either `result` or `error` (a string):

```json
{"id": 1, "result": {"message": "4 selected", "selected": 4}}
{"id": 2, "error": "unknown method `ui.zoom`"}
```

A line that isn't JSON gets `{"id": null, "error": "bad JSON: …"}`. Blank lines are ignored.
Requests run on the UI thread between frames, in the order they arrive.

## Methods

| Method            | Params                                         | Result                                                                |
|-------------------|------------------------------------------------|-----------------------------------------------------------------------|
| `engine.execute`  | `command`, `params` (object, default `{}`)     | The command's result, as from the ribbon                              |
| `engine.commands` | none                                           | Every command: `id`, `label`, `ribbon`, `shortcut`, `params`          |
| `ui.state`        | none                                           | `tab`, `pane`, `tool`, `table`, `active`, `extent`, `scale`, `status` |
| `ui.set`          | `tab?`, `pane?`, `tool?`, `table?`, `active?`  | The new `ui.state`                                                    |
| `ui.screenshot`   | `path`                                         | `{"path"}` once the window's next frame is saved as PNG               |
| `ui.render`       | `path`, `width?`, `height?` (1–8192, 1280×800) | `{"path"}`: the map alone, rendered offscreen as Export Map writes it |

`ui.set` values:

| Param    | Values                                                    |
|----------|-----------------------------------------------------------|
| `tab`    | `map`, `analysis`, `statistics`, `edit`, `view`, `share`  |
| `pane`   | `geoprocessing`, `symbology`, `history`, `none` (or `""`) |
| `tool`   | `select`; anything else is `explore`                      |
| `table`  | `true` or `false`: the attribute table shows              |
| `active` | a layer id                                                |

`ui.screenshot` needs a presented frame, so it waits for the window to draw; `ui.render` doesn't
and also works while the window is hidden.

## Example

```sh
printf '%s\n' \
  '{"id":1,"method":"engine.execute","params":{"command":"map.selectByAttributes","params":{"layer":"Cidades","where":"POP > 100000"}}}' \
  '{"id":2,"method":"ui.render","params":{"path":"/tmp/map.png"}}' \
  | nc -q 1 127.0.0.1 7979
```

## Revision history

| Date       | Change              |
|------------|---------------------|
| 2026-10-21 | First version (#62) |
