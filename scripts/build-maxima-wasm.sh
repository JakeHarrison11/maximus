#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EMSDK_DIR="$ROOT/build/emsdk"
ECL_HOST_DIR="$ROOT/build/ecl-host"
ECL_WASM_DIR="$ROOT/build/ecl-wasm"
ECL_SRC="$ROOT/build/ecl-26.3.27"
MAXIMA_SRC="$ROOT/build/maxima-code"
HELP_INFO=""
OUT_DIR="$ROOT/crates/maximus-app/web/runtime"
PACKAGES_MODE="all"
PACKAGES=()
ALLOW_UNPINNED=0

usage() {
    cat <<'EOF'
Build the ECL/Maxima browser runtime from locally prepared source/toolchains.

Usage: scripts/build-maxima-wasm.sh [options]
  --emsdk DIR       Existing Emscripten SDK (default: build/emsdk)
  --ecl-host DIR    Existing native ECL install (default: build/ecl-host)
  --ecl-wasm DIR    Existing Emscripten ECL build (default: build/ecl-wasm)
  --ecl-src DIR     ECL source for library licenses (default: build/ecl-26.3.27)
  --maxima-src DIR  Maxima source checkout (default: build/maxima-code)
  --help-info PATH  Full English Maxima Info file (otherwise autodetected)
  --package NAME   Include only this share package; repeatable
  --no-packages    Do not stage optional share package assets
  --allow-unpinned Permit Emscripten/ECL versions other than pinned baseline
  -h, --help       Show this help

No source or toolchain downloads are performed. Prepare the inputs yourself.
The default optional-package mode stages all Maxima share packages under
runtime/packages/<Maxima-version>/; they are not part of the eager core bundle.
EOF
}

while (($#)); do
    case "$1" in
        --emsdk) EMSDK_DIR="${2:?--emsdk requires a directory}"; shift 2 ;;
        --ecl-host) ECL_HOST_DIR="${2:?--ecl-host requires a directory}"; shift 2 ;;
        --ecl-wasm) ECL_WASM_DIR="${2:?--ecl-wasm requires a directory}"; shift 2 ;;
        --ecl-src) ECL_SRC="${2:?--ecl-src requires a directory}"; shift 2 ;;
        --maxima-src) MAXIMA_SRC="${2:?--maxima-src requires a directory}"; shift 2 ;;
        --help-info) HELP_INFO="${2:?--help-info requires a file}"; shift 2 ;;
        --package)
            if [[ "$PACKAGES_MODE" == "all" ]]; then PACKAGES_MODE="selected"; fi
            PACKAGES+=("${2:?--package requires a name}")
            shift 2
            ;;
        --no-packages) PACKAGES_MODE="none"; PACKAGES=(); shift ;;
        --allow-unpinned) ALLOW_UNPINNED=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

require_file() {
    [[ -f "$1" ]] || { echo "Required input file not found: $1" >&2; exit 1; }
}
require_dir() {
    [[ -d "$1" ]] || { echo "Required input directory not found: $1" >&2; exit 1; }
}

require_file "$EMSDK_DIR/emsdk_env.sh"
require_file "$ECL_HOST_DIR/bin/ecl"
require_file "$ECL_SRC/contrib/asdf/asdf.lisp"
require_file "$ECL_WASM_DIR/help.doc"
require_file "$ECL_WASM_DIR/libecl.a"
require_file "$ECL_WASM_DIR/libeclgc.a"
require_file "$ECL_WASM_DIR/libeclgmp.a"
require_file "$ECL_WASM_DIR/ecl/config-internal.h"
require_file "$ECL_WASM_DIR/target-info.lsp"
if grep -q '^#define ECL_CAN_SET_STACK_SIZE' "$ECL_WASM_DIR/ecl/config-internal.h"; then
    echo "ECL WASM still probes OS stack limits; rebuild with scripts/build-ecl-wasm.sh." >&2
    exit 1
fi
require_dir "$MAXIMA_SRC/src"
require_file "$MAXIMA_SRC/src/maxima-package.lisp"
require_file "$MAXIMA_SRC/COPYING"
require_file "$ECL_SRC/LICENSE"
require_file "$ECL_SRC/src/gmp/COPYING.LIB"
require_file "$ECL_SRC/src/bdwgc/include/gc.h"
require_file "$ROOT/crates/maximus-backend-native/src/driver.lisp"
command -v sha256sum >/dev/null || { echo "sha256sum is required" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

# The verified prototype baseline is Emscripten 4.0.15 + ECL 26.3.27.
# Record all tool/input fingerprints in build-info.json; never silently fetch
# or replace a toolchain. --allow-unpinned is an explicit opt-in for upgrades.
export EMSDK_QUIET=1
source "$EMSDK_DIR/emsdk_env.sh" >/dev/null 2>&1
command -v emcc >/dev/null || { echo "emcc was not provided by $EMSDK_DIR" >&2; exit 1; }
command -v node >/dev/null || { echo "node is required by Emscripten" >&2; exit 1; }
EMCC_VERSION="$(emcc --version | sed -n '1s/.*) \([0-9][^ ]*\).*/\1/p')"
ECL_VERSION="$("$ECL_HOST_DIR/bin/ecl" --version 2>&1 | sed -n '1s/^ECL //p')"
if (( ! ALLOW_UNPINNED )); then
    [[ "$EMCC_VERSION" == "4.0.15" ]] || {
        echo "Expected Emscripten 4.0.15, found ${EMCC_VERSION:-unknown}; pass --allow-unpinned to override." >&2
        exit 1
    }
    [[ "$ECL_VERSION" == "26.3.27" ]] || {
        echo "Expected ECL 26.3.27, found ${ECL_VERSION:-unknown}; pass --allow-unpinned to override." >&2
        exit 1
    }
fi

if [[ -n "$HELP_INFO" ]]; then
    require_file "$HELP_INFO"
fi

MAXIMA_VERSION="$(git -C "$MAXIMA_SRC" describe --tags --always --dirty 2>/dev/null || printf 'source-tree')"
if [[ "$PACKAGES_MODE" == "all" ]]; then
    mapfile -t PACKAGES < <(find "$MAXIMA_SRC/share" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | LC_ALL=C sort)
elif [[ "$PACKAGES_MODE" == "none" ]]; then
    PACKAGES=()
fi

OUT_DIR="$(mkdir -p "$OUT_DIR" && cd "$OUT_DIR" && pwd)"
mkdir -p "$OUT_DIR/licenses"
rm -rf "$OUT_DIR/packages"
"$ECL_HOST_DIR/bin/ecl" -norc -shell "$ROOT/scripts/maxima-load-plan.lisp" \
    "$MAXIMA_SRC/" "$OUT_DIR/load-plan.txt"
[[ -s "$OUT_DIR/load-plan.txt" ]] || { echo "Maxima source-load plan is empty" >&2; exit 1; }
mkdir -p "$ROOT/build"
AOT_DIR="$(mktemp -d "$ROOT/build/maxima-aot.XXXXXX")"
trap 'rm -r "$AOT_DIR"' EXIT
"$ECL_HOST_DIR/bin/ecl" -norc -shell "$ROOT/scripts/compile-maxima-wasm.lisp" \
    "$MAXIMA_SRC/" "$ECL_WASM_DIR/target-info.lsp" "$AOT_DIR/" "$OUT_DIR/load-plan.txt"

INFO_ARGS=()
if [[ -n "$HELP_INFO" ]]; then
    INFO_ARGS=(--help-info "$HELP_INFO")
fi
PACKAGE_ARGS=()
for package in "${PACKAGES[@]}"; do
    [[ "$package" =~ ^[A-Za-z0-9._-]+$ ]] || {
        echo "Invalid package name: $package" >&2
        exit 2
    }
    [[ -d "$MAXIMA_SRC/share/$package" ]] || {
        echo "Maxima share package not found: $package" >&2
        exit 1
    }
    PACKAGE_ARGS+=(--package "$package")
done

# ECL's browser port, socket stubs, and CMP omission are defined in
# runtime/boot.lisp. The native driver's protocol stays the single source of
# truth and is embedded at /driver.lisp for browser calls.
EMCC_FLAGS=(
    "$ROOT/crates/maximus-backend-web/runtime/embed.c"
    "-I$AOT_DIR"
    "$AOT_DIR/maxima"
    "-I$ECL_WASM_DIR" "-L$ECL_WASM_DIR"
    -lecl -leclgc -leclgmp -lm
    -O2
    -fwasm-exceptions
    -sSUPPORT_LONGJMP=wasm
    -sSTACK_SIZE=8388608
    -sALLOW_MEMORY_GROWTH=1
    -sBINARYEN_EXTRA_PASSES=--spill-pointers,--coalesce-locals
    -sMODULARIZE=1
    -sEXPORT_ES6=1
    -sENVIRONMENT=web,worker,node
    -sEXPORTED_RUNTIME_METHODS=ccall,FS
    -sEXPORTED_FUNCTIONS=_initialize_maxima,_load_maxima_file,_finish_maxima,_evaluate_lisp,_evaluate_maxima,_list_variables,_malloc,_free
    --preload-file "$MAXIMA_SRC/src@/maxima/src"
    --preload-file "$MAXIMA_SRC/lisp-utils@/maxima/lisp-utils"
    --preload-file "$OUT_DIR/load-plan.txt@/load-plan.txt"
    --preload-file "$ECL_WASM_DIR/help.doc@/ecl/help.doc"
    --preload-file "$ROOT/crates/maximus-backend-web/runtime/boot.lisp@/boot.lisp"
    --preload-file "$ROOT/crates/maximus-backend-native/src/driver.lisp@/driver.lisp"
    -o "$OUT_DIR/maxima.mjs"
)

echo "Building Maxima WASM with Emscripten $EMCC_VERSION and ECL $ECL_VERSION"
emcc "${EMCC_FLAGS[@]}"

write_help() {
    local help_dir="$1"
    local target="$OUT_DIR/help.info"
    if [[ -n "$HELP_INFO" ]]; then
        cp "$HELP_INFO" "$target"
    elif [[ -d "$help_dir" ]] && compgen -G "$help_dir/maxima.info-[0-9]*" >/dev/null; then
        python3 "$ROOT/scripts/combine-maxima-info.py" "$help_dir" "$target"
    elif [[ -f "$MAXIMA_SRC/doc/info/maxima.info" ]]; then
        cp "$MAXIMA_SRC/doc/info/maxima.info" "$target"
    else
        echo "Full Maxima Info manual not found; pass --help-info PATH or install maxima.info chunks." >&2
        exit 1
    fi
    grep -q 'Node: Top' "$target" || {
        echo "Generated help.info does not contain a Maxima Info Top node" >&2
        exit 1
    }
}

write_help "${MAXIMA_INFO_DIR:-/usr/share/info}"
cp "$MAXIMA_SRC/COPYING" "$OUT_DIR/licenses/maxima-COPYING"
cp "$ECL_WASM_DIR/COPYING" "$OUT_DIR/licenses/ECL-COPYING"
cp "$ECL_SRC/LICENSE" "$OUT_DIR/licenses/ECL-LICENSE"
cp "$ECL_SRC/src/gmp/COPYING.LIB" "$OUT_DIR/licenses/GMP-COPYING.LIB"
cp "$ECL_SRC/src/bdwgc/include/gc.h" "$OUT_DIR/licenses/Boehm-GC-gc.h"
cp "$EMSDK_DIR/LICENSE" "$OUT_DIR/licenses/emscripten-LICENSE"

python3 "$ROOT/scripts/write-web-runtime-metadata.py" \
    --runtime "$OUT_DIR" \
    --maxima-src "$MAXIMA_SRC" \
    --maxima-version "$MAXIMA_VERSION" \
    --emsdk-version "$EMCC_VERSION" \
    --ecl-version "$ECL_VERSION" \
    --ecl-wasm "$ECL_WASM_DIR" \
    --ecl-host "$ECL_HOST_DIR" \
    --ecl-src "$ECL_SRC" \
    "${INFO_ARGS[@]}" \
    "${PACKAGE_ARGS[@]}"

echo "WASM runtime written to $OUT_DIR"
