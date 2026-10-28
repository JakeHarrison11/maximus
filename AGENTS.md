# Maximus – guide for agents and new contributors

Maximus is a Rust + egui notebook frontend for the Maxima CAS: cells, textbook (2D) input and output, integrated help, plotting with `egui_plot`, a minimal calculator mode, an MCP server and TOML plugins. [README.md](README.md) covers the user-facing features; this file covers how the code is organised and how to work on it.

## Quick start

```
cargo build -p maximus-app                       # debug build
cargo run -p maximus-app -- [--minimal] [--run] [file.maximus]
cargo run -p maximus-app -- --mcp                # headless MCP server on stdio
cargo test --workspace                           # ~1 min; some tests start a real Maxima
cargo check -p maximus-app --target wasm32-unknown-unknown   # keep the web build compiling
```

Requirements: Rust (edition 2021), Maxima (`maxima` on PATH, tested with 5.49 on sbcl; the Info manual is needed for help), optionally gnuplot. The `wasm32-unknown-unknown` target must be installed for the WASM check.

Before finishing any change: `cargo test --workspace` and the WASM check must pass. Do not commit screenshots (`*.png` is git-ignored).

## Crates (dependency order, lowest first)

| crate | what lives there |
| --- | --- |
| `maximus-math` | The math model and all conversions. `tree.rs`: `Item`/`Row` textbook tree, `from_latex`, `row_to_latex`. `semantic.rs`: `parse_tree`/`tree_to_maxima` (tree → Maxima). `maxima_parse.rs` + `expr.rs`: Maxima text → `Expr` → tree (`maxima_to_tree`, `split_statements`, `maxima_script_to_tree/latex`). `words.rs`: function-name registry; names shorter than 4 letters count as functions only when followed by `(`. |
| `maximus-core` | `Notebook`/`Cell` (JSON `.maximus` files), the `Backend` trait, `Session` (evaluation queue, buffered output, variable tracking via `user_names`/`defined_names`), `PlotData`, plugin manifests. No GUI and no process code, so it builds for WASM. |
| `maximus-backend-native` | Spawns Maxima (`--preload-lisp=driver.lisp -r ':lisp (maxima::mx-loop)'`) and talks a framed, tab-separated `@@MX` protocol with `driver.lisp`. Each backend gets its own temp driver dir. Plot sampling happens in Lisp and comes back as JSON. |
| `maximus-help` | Parses the Maxima Info manual into a searchable index (handles the newer indented-signature format) and reflows prose. |
| `maximus-render` | `layout.rs`: textbook layout engine (boxes, baselines, hand-drawn integral sign). `editor.rs`: `EditorState` (cursor, anchor, selection as `Part::{Items,Open,Close}`, lone brackets, matrix ops). `editor_ui.rs`: the `math_editor` widget and read-only `math_view`. Bundled fonts are in `assets/` (Latin Modern, Liberation Serif Italic, DejaVu). |
| `maximus-plot` | `show_plot` (egui_plot with hover snapping, coordinates, zoom buttons), critical-point detection, optional gnuplot renderer. |
| `maximus-mcp` | JSON-RPC MCP server (tools: `evaluate`, `latex_to_maxima`, `maxima_to_latex`, `list_variables`, `reset`, `help`, `search_help`, `plot`, `notebook_*`). |
| `maximus-app` | The egui app and the `maximus` binary. `app.rs`: state, menus, window/sidebar/zoom management, file dialogs (rfd), single-selection enforcement. `cells.rs`: notebook UI, per-cell input modes, selectable typeset output, result context menu. `side.rs`: Help / Variables / Symbols / Functions tabs. `minimal.rs`: calculator layout and keypad. `catalog.rs`: Maxima menu and symbol palettes. `icons.rs`: shape-drawn icons. `settings.rs`: persisted settings. `lib.rs`: `run_native` / WASM `start`. |

Other files: `examples/demo.maximus` (guided tour; a test evaluates every cell, so keep it valid), `examples/plugins/physics` (sample plugin).

## Data flow

1. A code cell stores one of three input forms: textbook (`Row` tree), Maxima source, or LaTeX source. Switching forms converts via `maximus-math`.
2. Evaluating turns the cell into a Maxima string (`tree_to_maxima`) and sends it through `Session` to a `Backend`.
3. The reply is Maxima text; `maxima_to_tree` produces the typeset output tree shown by `maximus-render`. Output is buffered and replaced only when the new evaluation finishes, so re-evaluating never makes neighbouring cells flicker.
4. Textbook input and `integrate(x*sin(x)^3,x,0,inf)` must produce the same Maxima call; tests in `maximus-math` guard this.

## Where to change what

- New math syntax or a conversion bug: `maximus-math` (add a test next to the code first).
- Editor behaviour (cursor, selection, brackets, templates): `maximus-render/src/editor.rs` (model, unit-testable) and `editor_ui.rs` (input handling and painting).
- Menu entries and palettes: `maximus-app/src/catalog.rs`. Plugins can add more at runtime (`maximus-core/src/plugin.rs`, manifest format `plugin.toml`, folder from `$MAXIMUS_PLUGIN_DIR` or the config dir).
- New MCP tool: `maximus-mcp/src/lib.rs` (tool list and handler), then mention it in the README.
- Persisted preference: add a field to `Settings` in `settings.rs` (use `#[serde(default)]`) and, if needed, a control in the settings window in `app.rs`.

## Settings and files

Settings live in `<config dir>/maximus/settings.json` (`dirs::config_dir`, so `$XDG_CONFIG_HOME` is honoured; use a temp `XDG_CONFIG_HOME` when testing). Window size and maximized state are deliberately **not** persisted: the app starts at `settings::default_size` for the mode, and `manage_window` in `app.rs` only remembers each mode's size in memory so that switching back from minimal to full restores the previous size. Sizes are in **logical pixels**; egui reports **points** (logical pixels divided by the UI scale), so convert with `ctx.zoom_factor()`. Minimum window sizes are defined in points and scaled by UI scale (`settings::min_size`). On Wayland egui never reports `viewport().inner_rect`, so the code falls back to `ctx.viewport_rect()`. Do not send a redundant `ViewportCommand::Maximized` together with `InnerSize`: on Wayland it cancels the resize.

## Testing and visual checks

- Unit tests live next to the code. Tests that need Maxima skip or fail clearly when it is missing.
- UI changes should be looked at, not just compiled. There is no screenshot flag in the app. Use a virtual display, for example:
  1. `xvfb-run -a -s "-screen 0 1400x900x24 -fbdir /tmp/fb" script.sh`, where the script starts `WAYLAND_DISPLAY= XDG_CONFIG_HOME=/tmp/cfg XDG_DATA_HOME=/tmp/data target/debug/maximus`, sleeps ~6 s, and copies `/tmp/fb/Xvfb_screen0`.
  2. Convert the XWD file to PNG (a short Python script using `struct`/`zlib` is enough) and view it.
  3. `xdotool` can resize or move the window; there is no window manager under Xvfb, so min-size hints are not enforced there.
- Seed `/tmp/cfg/maximus/settings.json` to test specific modes, sizes or UI scales. Delete temp files afterwards.
- Never leave debugging hacks (forced selections, `eprintln!`) in committed code.

## egui 0.37 / eframe notes

- Panels: `Panel::right/bottom(...).show(ui, …)`; popups: `Popup::menu(&response).show`; theme: `ctx.set_theme(ThemePreference)`; unique ids: `Id::unique`.
- eframe is built with `default-features = false`; the `clipboard` and `links` features must stay enabled or copy/paste silently uses an internal clipboard.
- `ScrollArea` drag-to-scroll is touch-only by default.
- Wide labels or nested right-aligned layouts can make panels grow: prefer `ui.set_max_width`, `horizontal_wrapped`, and avoid nesting `with_layout(Align::Max)`.
- `with_inner_size`, `ViewportCommand::InnerSize` and `MinInnerSize` use logical pixels/points as described above.
- Icons are drawn as shapes in `icons.rs` (no font dependence); the plus signs on buttons are font glyphs by request.

## Conventions

- Keep `maximus-math`, `-core`, `-render`, `-plot` and `-help` free of native-only dependencies (processes, threads, files) so WASM keeps building. Native code goes in `maximus-backend-native` or behind `cfg(not(target_arch = "wasm32"))` in the app.
- Comment only what needs clarification. Prefer small focused tests over large ones.
- Commit messages are short imperative sentences describing the behaviour change.

## Known gaps

- The browser build has no Maxima backend yet (needs a remote or WASM-compiled Maxima behind the `Backend` trait).
- The rfd file dialogs and the clipboard paste path have only been exercised in limited environments (Xvfb, no real desktop portal).
- Plugins are intentionally lightweight (manifest, Maxima files, menu items, palettes, snippets); there is no native code loading.
