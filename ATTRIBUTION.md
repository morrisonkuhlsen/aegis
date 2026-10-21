# Attribution

Every asset file in this repository — images, icons, fonts, data, styles — has a row here: what it
is, who made it, where it came from, and its licence. `cargo xtask assets` fails on a file under
`assets/`, `crates/ui-egui/assets/` or `docs/brand/` without a row.

mkaeGIS draws its art in code, so this table is short:

| Asset                     | Author               | Source                                                                                                                                                     | Licence           |
|---------------------------|----------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------|
| UI icons                  | mkaeGIS contributors | drawn in code, `crates/ui-egui/src/icons.rs`                                                                                                               | MIT OR Apache-2.0 |
| Colour ramps and palettes | mkaeGIS contributors | defined in code, `crates/model/src/symbology.rs`                                                                                                           | MIT OR Apache-2.0 |
| Sample map                | mkaeGIS contributors | generated in code, `crates/engine/src/sample.rs`                                                                                                           | MIT OR Apache-2.0 |
| Splash artwork            | Dante Bertuzzi       | `crates/ui-egui/assets/splash.jpg`; made by the contributor with an AI image generator, cropped to remove the generator's watermark and scaled to 1280×720 | MIT OR Apache-2.0 |
| README screenshots        | mkaeGIS contributors | `docs/images/map.png`, `docs/images/dark-analysis.png`; rendered from mkaeGIS's own sample map with the offscreen snapshot example                         | MIT OR Apache-2.0 |
