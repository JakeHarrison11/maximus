#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT/build/ecl-26.3.27"
EMSDK="$ROOT/build/emsdk"
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
HOST="$ROOT/build/ecl-host"
WASM="$ROOT/build/ecl-wasm"
HOST_BUILD="$ROOT/build/ecl-host-build"
WASM_BUILD="$ROOT/build/ecl-wasm-build"
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
# ECL does not invalidate all objects when compiler/longjmp settings change.
if [[ -f Makefile ]]; then
    make clean
fi
# ECL's pinned configure forces -O0 on WASM. Optimized frames are important on
# Safari, whose execution stack is separate from Emscripten's linear-memory stack.
sed -e 's/CFLAGS="${CFLAGS} -DECL_C_COMPATIBLE_VARIADIC_DISPATCH -O0"/CFLAGS="${CFLAGS} -DECL_C_COMPATIBLE_VARIADIC_DISPATCH -O2 -fwasm-exceptions -sSUPPORT_LONGJMP=wasm"/' \
    -e 's/if test "x${cross_compiling}" = "xyes"; then/if test "x${host}" = "xwasm32-unknown-emscripten" || test "x${cross_compiling}" = "xyes"; then/' \
    "$SOURCE/src/configure" > configure-wasm
grep -q 'CFLAGS="${CFLAGS} -DECL_C_COMPATIBLE_VARIADIC_DISPATCH -O2 -fwasm-exceptions -sSUPPORT_LONGJMP=wasm"' configure-wasm || {
    echo "Expected ECL 26.3.27 WASM compiler flags were not found" >&2
    exit 1
}
# Emscripten has RLIMIT_STACK headers but no working prlimit64 syscall.
ac_cv_have_decl_RLIMIT_STACK=no ECL_TO_RUN="$HOST/bin/ecl" LDFLAGS="-fwasm-exceptions -sSUPPORT_LONGJMP=wasm" \
    emconfigure bash "$WASM_BUILD/configure-wasm" --srcdir="$SOURCE/src" \
    --host=wasm32-unknown-emscripten --build="$(bash "$SOURCE/src/config.guess")" \
    --with-cross-config="$SOURCE/src/util/wasm32-unknown-emscripten.cross_config" \
    --prefix="$WASM" --disable-shared --with-tcp=no --with-cmp=no --with-asdf=no
make -j"$JOBS"
make install bindir="$WASM"
cp "$SOURCE/contrib/asdf/asdf.lisp" "$WASM/asdf.lisp"
cp "$WASM_BUILD/target-info.lsp" "$WASM/target-info.lsp"
for library in libecl.a libeclgc.a libeclgmp.a asdf.lisp help.doc; do
    [[ -s "$WASM/$library" ]] || { echo "Missing or empty ECL artifact: $library" >&2; exit 1; }
done
echo "ECL installed in $HOST and $WASM"
