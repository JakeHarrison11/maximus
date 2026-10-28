# Maximus

A textbook-style notebook frontend for the [Maxima](https://maxima.sourceforge.io/) computer algebra system, written in Rust with [egui](https://github.com/emilk/egui).

## Features

- **Cells** – code, text, section and title cells that can be re-ordered, duplicated, collapsed and re-evaluated without touching the Maxima session. Notebooks are saved as `.maximus` files and can be exported as LaTeX or a Maxima script.
- **Textbook input** – a structured 2D editor (fractions, powers, roots, integrals, sums, limits, matrices). `\int_0^{\infty}\!x\sin^3(x)\,\mathrm{d}x` and `integrate(x*sin(x)^3,x,0,inf)` produce the same Maxima call. Every cell can also be switched to a plain Maxima or LaTeX source view; LaTeX and Maxima text can be pasted, and the input can be copied as LaTeX or Maxima.
- **Textbook output** – results are laid out as typeset math (fractions, radicals, big operators, matrices).
- **Integrated help** – the side panel follows the function under the cursor, right-click a function for its documentation, and browse or search every function of the Maxima manual. Most Maxima functions are reachable from the *Maxima* menu through argument dialogs.
- **Symbol palette** – Greek letters, operators and structures, plus palettes from plugins.
- **Variables panel** – view, edit, delete and reset all user variables and functions.
- **Plotting** – `plot2d` and `plot3d` are sampled in Maxima and drawn with `egui_plot`; gnuplot can be selected in Settings.
- **Minimal mode** – a calculator-like view with a keypad (`Ctrl+M`, or `--minimal`). The full CAS stays available.
- **MCP server** – `maximus --mcp` speaks the Model Context Protocol over stdio (tools: `evaluate`, `latex_to_maxima`, `maxima_to_latex`, `list_variables`, `reset`, `help`, `search_help`, `plot`, `notebook_*`).
- **Plugins** – folders with a `plugin.toml` in `~/.config/maximus/plugins` (or `$MAXIMUS_PLUGIN_DIR`) can load Maxima files and add menu entries, symbol palettes and snippets. See [examples/plugins/physics](examples/plugins/physics).
- **WASM-ready architecture** – `maximus-math`, `maximus-core`, `maximus-render`, `maximus-plot` and `maximus-help` build for `wasm32-unknown-unknown`; the GUI crate does too. Maxima runs behind the `Backend` trait, so the browser build needs a remote or WASM-compiled Maxima backend (not yet implemented; the web build starts with a "no backend" notice).

## Build and run

Requires Rust and Maxima (the `maxima` executable and, for help, its Info manual).

```
cargo run --release -p maximus-app -- [--minimal] [--run] [notebook.maximus]
cargo run --release -p maximus-app -- --mcp
cargo test --workspace
```

A guided tour lives in [examples/demo.maximus](examples/demo.maximus): `cargo run --release -p maximus-app -- --run examples/demo.maximus`.

Keyboard: `Enter` evaluates a textbook cell, `Shift+Enter` evaluates a Maxima/LaTeX-source cell, `Ctrl+Shift+Enter` evaluates everything, `Ctrl+.` interrupts, `Ctrl+S` / `Ctrl+O` save and open. In the editor, select with Shift+arrows, click-drag, double-click (word) or `Ctrl+A`; typing replaces the selection, and `/`, `(`, `[`, `{`, `|` or the Fraction/Root buttons wrap it. `Ctrl+C`/`Ctrl+X` copy or cut the selection as LaTeX. `/` makes a fraction, `^` a power, `_` a subscript, `sqrt(` a root.

## Layout

| crate | role |
| --- | --- |
| `maximus-math` | math tree, LaTeX ⇄ tree ⇄ Maxima conversion |
| `maximus-core` | notebook, `Backend` trait, evaluation session, plugin manifests |
| `maximus-backend-native` | drives a local Maxima process through a Lisp driver |
| `maximus-render` | textbook layout engine, 2D editor model and widget |
| `maximus-help` | Maxima manual index |
| `maximus-plot` | `egui_plot` and gnuplot plotting |
| `maximus-mcp` | MCP server |
| `maximus-app` | the egui application (`maximus` binary) |

Web build (untested with trunk): `trunk serve crates/maximus-app/web/index.html`.
