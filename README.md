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
- **Minimal mode** – a touch-friendly calculator (`Ctrl+M`, the calculator/notebook icons, or `--minimal`). Large keys, tap-open trigonometry, math, calculus and variable menus, and left/right cursor arrows support textbook input without a physical keyboard. Variable labels use the editor's Latin Modern math rendering. Quick keys include **×10ˣ** (multiply by ten to a power, then enter the exponent), **π**, **x²**, and **!**. Trigonometric functions use radians. **≈**, directly above **=**, evaluates the input as a decimal, or converts the latest result when the input is empty; **÷** inserts a textbook fraction and **Ans** reuses the last answer. Click a history input to edit it again. The options menu can hide the keypad or clear history (Maxima variables are kept). On short windows the keypad scrolls, leaving the input and history accessible. The full CAS stays available.
  Minimal mode hides the menu bar and footer, keeping only a compact maximus heading with options and notebook controls. Empty history and input are left blank. Session status, messages, and Stop/Restart controls are available in its options menu. Clicking or tapping outside a Trig/Math/Calc/Vars menu only dismisses it, without activating controls underneath.
- **MCP server** – `maximus --mcp` speaks the Model Context Protocol over stdio (tools: `evaluate`, `latex_to_maxima`, `maxima_to_latex`, `list_variables`, `reset`, `help`, `search_help`, `plot`, `notebook_*`).
- **Plugins** – folders with a `plugin.toml` in `~/.config/maximus/plugins` (or `$MAXIMUS_PLUGIN_DIR`) can load Maxima files and add menu entries, symbol palettes and snippets. See [examples/plugins/physics](examples/plugins/physics).
- **Offline browser application** – the same notebook and calculator UI runs with a local ECL/Maxima WebAssembly engine in a dedicated Web Worker. No remote CAS, shared memory, or cross-origin isolation headers are needed. Notebook recovery, settings, uploaded CAS files, plugins, and explicitly installed Maxima packages persist in browser storage.

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

Contributors and coding agents: see [AGENTS.md](AGENTS.md) for architecture, workflows and gotchas.

| crate | role |
| --- | --- |
| `maximus-math` | math tree, LaTeX ⇄ tree ⇄ Maxima conversion |
| `maximus-core` | notebook, `Backend` trait, evaluation session, plugin manifests |
| `maximus-backend-native` | drives a local Maxima process through a Lisp driver |
| `maximus-backend-web` | local ECL/Maxima WASM worker transport and runtime bootstrap |
| `maximus-render` | textbook layout engine, 2D editor model and widget |
| `maximus-help` | Maxima manual index |
| `maximus-plot` | `egui_plot` and gnuplot plotting |
| `maximus-mcp` | MCP server |
| `maximus-app` | the egui application (`maximus` binary) |

## Browser build

The browser UI uses Rust's `wasm32-unknown-unknown` target. The separate CAS engine
uses **Emscripten 4.0.15** and **ECL 26.3.27**. Maxima is source-loaded using ECL's
bytecode interpreter, not a native Maxima executable or an ahead-of-time compiled
Maxima image. Startup and heavy calculations can be slower than the desktop build.
The tested Maxima source revision is
`83548a8c991d47f7bf3317d1d67aaf65a417a281` (5.50post).

Prepare these local inputs (build scripts never silently download toolchains):

- `temp/emsdk`: the [Emscripten SDK](https://github.com/emscripten-core/emsdk),
  with `./emsdk install 4.0.15` and `./emsdk activate 4.0.15` completed. The tested
  SDK manager revision is `389a68bc35dcff7ebae4614e1615099dafda00d1`.
- `temp/ecl-26.3.27`: unpack the 26.3.27 source release from
  [ECL](https://ecl.common-lisp.dev/). The tested archive SHA-256 is
  `416d5707bf11d2b3d8d33d6791419a786e4cc59ac0cc3ec505ee59b51a9f5c9a`.
- `temp/maxima-code`: the [Maxima source](https://sourceforge.net/p/maxima/code/).
- A full English Maxima Info manual: installed `maxima.info-*` chunks in
  `/usr/share/info`, a source-tree `doc/info/maxima.info`, or an explicit
  `--help-info /path/to/complete.info` argument to the runtime builder.
- Native C/C++ tools, `make`, `m4`, Python 3, Rust, and Trunk.

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
scripts/build-ecl-wasm.sh
scripts/build-maxima-wasm.sh
scripts/build-web.sh --skip-runtime --public-url ./
python3 -m http.server 8080 --directory dist/web
```

Open `http://localhost:8080/`. Do not open the HTML as a `file://` URL. The ECL
builder installs a native host compiler in `temp/ecl-host` and cross-built libraries
in `temp/ecl-wasm`. Each script supports `--help` for alternate input directories.
After Rust, JS, or HTML changes, reuse the engine with `build-web.sh --skip-runtime`;
after Lisp driver/bootstrap or C bridge changes, rebuild the runtime first.

Deploy the **entire** `dist/web` directory on an ordinary HTTPS static host. Use
`--public-url /your/subpath/` for an explicit deployment path, or `./` for a
relocatable bundle. The build generates a checksummed asset manifest and a
versioned service worker. Wait for **Offline ready** before disconnecting.
Updates wait for old tabs to close before activating; do not overwrite individual
files from different builds. Generated runtime assets and bundles are git-ignored.
Provenance is in `runtime/build-info.json`, and third-party notices are in
`runtime/licenses`. Distribution of the compiled Maxima/ECL engine must preserve
its GPL/LGPL notices and meet the corresponding-source/relinking requirements;
retain the pinned upstream sources and these build/embedding sources.

### Browser behavior and limits

- **Open** uploads a `.maximus` file; **Save** downloads it. LaTeX and Maxima
  exports also download files. Autosave is a recovery copy in IndexedDB, not a
  replacement for downloaded backups.
- Settings use localStorage; recovery, plugin/workspace files and installed
  package bytes use IndexedDB. Storage is origin-specific, quota-limited and
  subject to browser eviction/private-mode policies. Failures are reported.
  Use one editing tab per origin: concurrent recovery saves are last-writer-wins.
- Use **File → Import CAS files** to mount files under `/workspace`, for example
  `load("/workspace/functions.mac")`. **Help → Plugins** imports a folder with
  `plugin.toml` and its source files, using the same manifest format as desktop.
  Folder selection needs `webkitdirectory` support.
- **Maxima packages** in the browser footer installs optional share groups on
  demand, verifying size and SHA-256 before atomically saving them. Install
  `combinatorics`, then evaluate `load("combinatorics")`; group and entry-file
  names can differ (`physics` contains `units.mac`). Install any dependencies as
  well. Successfully installed groups work offline and survive Stop/reload.
- **Stop / Ctrl+. terminates and recreates the worker**, clearing CAS state and
  displayed outputs, but retaining notebook inputs and persisted files.
  Reopening/recovering a notebook also starts a fresh CAS; evaluate cells to
  reconstruct variables and `%`/Ans. This avoids stale answers after a reset.
- Built-in plots, variables, integrated help, notebook/textbook/LaTeX views and
  minimal mode work locally. Browser clipboard access requires the usual
  browser permissions and secure context. Stdio MCP, gnuplot, OS paths, shell
  commands, native sockets and native-code-only packages remain desktop-only.
  Interactive terminal questions have no stdin and fail rather than hanging.
  Files written by CAS code are worker-local and are not automatically exported
  or persisted.

### Browser tests

With the built app served on localhost:

```sh
cargo test --workspace
cargo check -p maximus-app --target wasm32-unknown-unknown
source temp/emsdk/emsdk_env.sh
node --test crates/maximus-app/web/*.test.mjs
npm ci --prefix scripts
cd scripts
npx playwright install chromium
node test-web.mjs
node test-web-packages.mjs
```

`MAXIMUS_WEB_URL` selects another served base URL (including a subpath). The
Playwright test uses the real production worker and canvas application: Unicode,
algebra/calculus, plotting, imports/downloads, recovery, Stop, and an offline
reload with networking disabled. `--ui-only` skips the direct-worker segment.

## Icon credits

The calculator and notebook mode buttons use the **calculator** and **book-open**
icons from [Font Awesome Free 6.7.2](https://github.com/FortAwesome/Font-Awesome/tree/6.7.2),
copyright 2024 Fonticons, Inc., under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
The bundled SVGs have their fill changed to white for theme-aware tinting; their
shapes are unchanged. See the [upstream license](crates/maximus-app/assets/fontawesome/LICENSE.txt).
Icons are embedded locally and require no network connection.
