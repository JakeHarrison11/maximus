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
Native integration tests exercise the Lisp driver with the installed Maxima;
its tab-separated protocol supports both SBCL and Ubuntu's GCL-based build.

The Flatpak package uses the Freedesktop 25.08 platform, SDK and Rust stable
extension. Keep their branches aligned in the packaging manifest and workflow;
the 24.08 Rust extension is too old for egui's Rust 1.95 minimum requirement.

The x86_64 Flatpak and AppImage bundle Maxima 5.50.0, an SBCL 2.5.11-based
engine, share packages and the English manual. Flatpak uses a separate Lisp
core file so packaging tools cannot strip it from the SBCL executable;
AppImage preserves its standalone executable unchanged. No host Maxima or
Lisp installation is required. Gnuplot is not bundled; the built-in plot
renderer works without it. Maxima's corresponding source archive and both
projects' license notices are included under `usr/share/maximus/sources` and
`usr/share/licenses` in the AppImage (under `/app/share` in Flatpak).

The AppImage includes the logo but does not install a desktop launcher;
Wayland panel/menu integration requires an external AppImage integration tool.
Flatpak exports the launcher and logo automatically. The desktop entry matches both the Wayland app ID
and the `io.github.jakeharrison11.Maximus` X11/XWayland window class.

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

### GitHub Pages deployment

The [Pages workflow](.github/workflows/pages.yml) builds and tests pull requests
and pushes to `master`. Only successful `master` builds deploy. Manual runs are
also available from the Actions tab.

To enable deployment, choose **Settings > Pages > Build and deployment > Source:
GitHub Actions** in this repository, then push to `master` or run the workflow.
The expected project URL is `https://JakeHarrison11.github.io/maximus/`; the
deployment job reports the actual URL. Pages provides HTTPS, allowing offline
installation on iPhone. Wait for **Offline ready** before disconnecting.

CI installs pinned Emscripten 4.0.15, verifies the ECL 26.3.27 release archive
against its SHA-256, and checks out the pinned Maxima and SDK-manager revisions.
It caches Emscripten, native/WASM ECL, Rust dependencies and Trunk, but rebuilds
Maxima from current build sources. Native tests, WASM checks, JavaScript tests,
the full Chromium suite and small-stack WebKit tests must pass before deployment.
Pull requests produce a downloadable `maximus-web` artifact without Pages write
permissions. The bundle uses relative URLs and is tested beneath `/web/`, so
project Pages paths and custom domains do not require different builds.

The deployment includes corresponding source archives under `sources/`:
`ecl.tgz`, `maxima.tar.gz` and `maximus.tar.gz`. These are not downloaded by the
offline installer. Keep these archives and the bundled license notices available
when redistributing the engine.

**Submodules:** pinned downloads are used instead. ECL has a checksummed release
archive, Emscripten needs its SDK installer regardless, and normal contributors
do not need large engine checkouts. Submodules would be reasonable if upstream
sources become actively patched or contributors need coordinated upstream work,
but introduce recursive-clone/update requirements without speeding up builds.
All current pins live in the workflow and must be updated deliberately together
with the tested build scripts.

### Local browser build

Toolchain checkouts and generated engine build files live in the git-ignored
`build/` directory. Existing checkouts elsewhere can still be used through the
build scripts' directory options.

The browser UI uses Rust's `wasm32-unknown-unknown` target. The separate CAS engine
uses **Emscripten 4.0.15** and **ECL 26.3.27**. Native host ECL cross-compiles most
Maxima Lisp files to C and then WASM during the build. Browser startup initializes
these linked modules instead of compiling their source. Browser shims and two
formatting-macro files still use ECL's bytecode interpreter; optional packages
can still load Lisp/Maxima source.
The tested Maxima source revision is
`83548a8c991d47f7bf3317d1d67aaf65a417a281` (5.50post).

Prepare these local inputs (build scripts never silently download toolchains):

- `build/emsdk`: the [Emscripten SDK](https://github.com/emscripten-core/emsdk),
  with `./emsdk install 4.0.15` and `./emsdk activate 4.0.15` completed. The tested
  SDK manager revision is `389a68bc35dcff7ebae4614e1615099dafda00d1`.
- `build/ecl-26.3.27`: unpack the 26.3.27 source release from
  [ECL](https://ecl.common-lisp.dev/). The tested archive SHA-256 is
  `416d5707bf11d2b3d8d33d6791419a786e4cc59ac0cc3ec505ee59b51a9f5c9a`.
- `build/maxima-code`: the [Maxima source](https://sourceforge.net/p/maxima/code/).
- A full English Maxima Info manual: installed `maxima.info-N` or
  `maxima.info-N.gz` chunks (combined in numeric order) in
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
builder installs a native host compiler in `build/ecl-host` and cross-built libraries
in `build/ecl-wasm`. Each script supports `--help` for alternate input directories.
After Rust, JS, or HTML changes, reuse the engine with `build-web.sh --skip-runtime`;
after Lisp driver/bootstrap or C bridge changes, rebuild the runtime first.
The ECL builder disables OS stack-limit probing (`prlimit64` is unsupported in
Emscripten) and overrides upstream's WASM `-O0` with `-O2` to reduce execution-stack
pressure in Safari. Changing these ECL settings requires rebuilding ECL, the
Maxima runtime, and the browser bundle, not just `build-web.sh --skip-runtime`.
The runtime also uses native WASM exception-based longjmp and compacts locals
after GC pointer spilling to keep execution frames small. ASDF resolves the
Maxima source-load order on the host at build time; browser startup loads the
resulting files in separate WASM calls rather than source-loading ASDF.
The ECL install includes `target-info.lsp`, required by the Maxima cross-compiler.
If this file is missing in an older ECL install, rerun `build-ecl-wasm.sh`.
The runtime builder uses `scripts/compile-maxima-wasm.lisp` to generate the
compiled module archive and initializer dispatch, and fails explicitly on
compilation errors. Original sources remain available for autoload compatibility.
The native WASM binary is approximately 34 MB uncompressed (5.2 MiB with gzip);
enable HTTP compression on the static host to reduce first-visit transfer time.
The footer shows startup progress and a timing breakdown for WASM download/
initialization, ECL boot, Maxima module initialization, finalization and persisted
storage. Offline-cache status is displayed separately. If mobile startup is
slow, compare the first visit and a reload after **Offline ready**; the breakdown
distinguishes network/engine setup from module initialization. A precompiled-bytecode
experiment improved desktop WebKit timings but doubled the data download and
regressed on iPhone, so that experiment was removed in favor of native WASM
compilation. Local small-stack WebKit cold startup measured approximately
one second with the native runtime; physical iPhone and network timings vary.

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
source build/emsdk/emsdk_env.sh
node --test crates/maximus-app/web/*.test.mjs
npm ci --prefix scripts
cd scripts
npx playwright install chromium
node test-web-runtime.mjs
node test-web.mjs
node test-web-packages.mjs
npx playwright install webkit
MAXIMUS_TEST_BROWSER=webkit node test-web-runtime.mjs
MAXIMUS_TEST_BROWSER=webkit node test-web.mjs --minimal-only
```

`MAXIMUS_WEB_URL` selects another served base URL (including a subpath). The
Playwright test uses the real production worker and canvas application: Unicode,
algebra/calculus, plotting, imports/downloads, recovery, Stop, and an offline
reload with networking disabled. `--ui-only` skips the direct-worker segment.
The runtime smoke test also runs in WebKit (Safari's engine), checking startup,
calculus and arithmetic, and rejecting stack-overflow/`prlimit64` diagnostics.
WebKit runs this test with a 256 KiB execution-stack limit to cover mobile stack
pressure instead of relying on desktop defaults.
This is not a substitute for testing on a physical iPhone.
The runtime test reports worker startup time. The application footer reports
the phase breakdown; on a narrow screen, scroll the footer horizontally to
read the full timing line.

## Icon credits

The calculator and notebook mode buttons use the **calculator** and **book-open**
icons from [Font Awesome Free 6.7.2](https://github.com/FortAwesome/Font-Awesome/tree/6.7.2),
copyright 2024 Fonticons, Inc., under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
The bundled SVGs have their fill changed to white for theme-aware tinting; their
shapes are unchanged. See the [upstream license](crates/maximus-app/assets/fontawesome/LICENSE.txt).
Icons are embedded locally and require no network connection.
