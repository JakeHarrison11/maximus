#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist/web"
PUBLIC_URL=""
SKIP_RUNTIME=0
RUNTIME_ARGS=()

usage() {
    cat <<'EOF'
Build the Maximus WASM web bundle and its offline asset manifest.

Usage: scripts/build-web.sh [options]
  --dist DIR         Output directory (default: dist/web)
  --public-url PATH  Trunk public URL for deployments below a base path
  --skip-runtime     Reuse an already generated web/runtime runtime
  --no-packages      Build without optional Maxima share packages
  --package NAME     Stage only a named Maxima share package (repeatable)
  --debug            Build the Rust/WASM application without release optimizations
  -h, --help         Show this help

Requires locally installed trunk, wasm32-unknown-unknown, and the inputs
described by scripts/build-maxima-wasm.sh. No dependencies are downloaded.
EOF
}

TRUNK_ARGS=(--release)
while (($#)); do
    case "$1" in
        --dist)
            DIST="${2:?--dist requires a directory}"
            [[ "$DIST" = /* ]] || DIST="$ROOT/$DIST"
            shift 2
            ;;
        --public-url) PUBLIC_URL="${2:?--public-url requires a path}"; shift 2 ;;
        --skip-runtime) SKIP_RUNTIME=1; shift ;;
        --no-packages) RUNTIME_ARGS+=(--no-packages); shift ;;
        --package) RUNTIME_ARGS+=(--package "${2:?--package requires a name}"); shift 2 ;;
        --debug) TRUNK_ARGS=(); shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

command -v trunk >/dev/null || {
    echo "trunk is required but was not found on PATH; install it explicitly, then retry." >&2
    exit 1
}
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

if (( ! SKIP_RUNTIME )); then
    "$ROOT/scripts/build-maxima-wasm.sh" "${RUNTIME_ARGS[@]}"
fi

RUNTIME="$ROOT/crates/maximus-app/web/runtime"
for artifact in maxima.mjs maxima.data maxima.wasm help.info packages.json; do
    [[ -f "$RUNTIME/$artifact" ]] || {
        echo "Missing $RUNTIME/$artifact; run scripts/build-maxima-wasm.sh first." >&2
        exit 1
    }
done

TRUNK_COMMAND=(trunk build web/index.html --dist "$DIST" "${TRUNK_ARGS[@]}")
if [[ -n "$PUBLIC_URL" ]]; then
    TRUNK_COMMAND+=(--public-url "$PUBLIC_URL")
fi
cd "$ROOT/crates/maximus-app"
"${TRUNK_COMMAND[@]}"

# Trunk copies referenced web resources; stage the complete generated runtime
# explicitly as well so runtime outputs need no hard-coded HTML asset list.
mkdir -p "$DIST/runtime"
cp -a "$RUNTIME/." "$DIST/runtime/"
cp "$ROOT/crates/maximus-app/web/service-worker.js" "$DIST/service-worker.js"

python3 "$ROOT/scripts/generate-asset-manifest.py" --dist "$DIST"
echo "Web bundle written to $DIST"
