#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT/temp/ecl-26.3.27"
EMSDK="$ROOT/temp/emsdk"
JOBS=2

while (($#)); do
    case "$1" in
        --ecl-src) SOURCE="${2:?--ecl-src requires a directory}"; shift 2 ;;
        --emsdk) EMSDK="${2:?--emsdk requires a directory}"; shift 2 ;;
        --jobs) JOBS="${2:?--jobs requires a positive integer}"; shift 2 ;;
        -h|--help)
            echo "Usage: scripts/build-ecl-wasm.sh [--ecl-src DIR] [--emsdk DIR] [--jobs N]"
            echo "Builds native and WASM ECL from prepared ECL 26.3.27/Emscripten 4.0.15 inputs."
            echo "No downloads; requires make, m4, a native C/C++ toolchain and Python."
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || { echo "Invalid job count: $JOBS" >&2; exit 2; }
SOURCE="$(realpath "$SOURCE")"
EMSDK="$(realpath "$EMSDK")"
[[ -f "$SOURCE/src/configure" && -f "$EMSDK/emsdk_env.sh" ]] || {
    echo "Missing prepared ECL source or Emscripten SDK" >&2
    exit 1
}
HOST="$ROOT/temp/ecl-host"
WASM="$ROOT/temp/ecl-wasm"
HOST_BUILD="$ROOT/temp/ecl-host-build"
WASM_BUILD="$ROOT/temp/ecl-wasm-build"
mkdir -p "$HOST_BUILD" "$WASM_BUILD"
cd "$HOST_BUILD"
"$SOURCE/src/configure" --srcdir="$SOURCE/src" --prefix="$HOST" \
    --enable-boehm=included --with-gmp=included --with-tcp=no --with-ffi=no
make -j"$JOBS"
make install
[[ "$("$HOST/bin/ecl" --version)" == "ECL 26.3.27" ]] || {
    echo "Expected ECL 26.3.27 source" >&2
    exit 1
}

export EMSDK_QUIET=1
source "$EMSDK/emsdk_env.sh" >/dev/null 2>&1
emcc --version | head -1 | grep -q ' 4\.0\.15 ' || {
    echo "Expected Emscripten SDK 4.0.15" >&2
    exit 1
}
export PATH="$HOST/bin:$PATH"
cd "$WASM_BUILD"
emconfigure "$SOURCE/src/configure" --srcdir="$SOURCE/src" \
    --host=wasm32-unknown-emscripten --build="$("$SOURCE/src/config.guess")" \
    --with-cross-config="$SOURCE/src/util/wasm32-unknown-emscripten.cross_config" \
    --prefix="$WASM" --disable-shared --with-tcp=no --with-cmp=no --with-asdf=no
make -j"$JOBS"
make install bindir="$WASM"
cp "$SOURCE/contrib/asdf/asdf.lisp" "$WASM/asdf.lisp"
for library in libecl.a libeclgc.a libeclgmp.a asdf.lisp help.doc; do
    [[ -s "$WASM/$library" ]] || { echo "Missing or empty ECL artifact: $library" >&2; exit 1; }
done
echo "ECL installed in $HOST and $WASM"
