#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:?Usage: build-bundled-maxima.sh APPDIR}"
mkdir -p "$DEST"
DEST="$(cd "$DEST" && pwd)"
mkdir -p "$ROOT/build"
WORK="$(mktemp -d "$ROOT/build/maxima-package.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

curl -fsSL --retry 3 https://downloads.sourceforge.net/project/sbcl/sbcl/2.5.11/sbcl-2.5.11-x86-64-linux-binary.tar.bz2 -o "$WORK/sbcl.tar.bz2"
curl -fsSL --retry 3 https://downloads.sourceforge.net/project/maxima/Maxima-source/5.50.0-source/maxima-5.50.0.tar.gz -o "$WORK/maxima.tar.gz"
echo "4f229e3e4106080820d095bb30ded0cd475a5c3202cbb8e580179d6b1cbaad2a  $WORK/sbcl.tar.bz2" | sha256sum -c -
echo "0bc4b5e11fe153ef20b24a3a816b668ece5378cc738fa24ca426b62fd6d8fc37  $WORK/maxima.tar.gz" | sha256sum -c -
tar -xjf "$WORK/sbcl.tar.bz2" -C "$WORK"
tar -xzf "$WORK/maxima.tar.gz" -C "$WORK"
(
    cd "$WORK/sbcl-2.5.11-x86-64-linux"
    INSTALL_ROOT="$WORK/bootstrap" sh install.sh
)
export SBCL_HOME="$WORK/bootstrap/lib/sbcl"
(
    cd "$WORK/maxima-5.50.0"
    ./configure --prefix="$WORK/prefix" --libdir="$WORK/prefix/lib" --infodir="$WORK/prefix/share/info" \
        --enable-sbcl --enable-sbcl-exec --with-sbcl="$WORK/bootstrap/bin/sbcl" \
        --disable-clisp --disable-gcl --disable-ecl --disable-build-docs
    make -j"${JOBS:-2}"
    make install
)
mkdir -p "$DEST/usr"
cp -a "$WORK/prefix/." "$DEST/usr/"
mkdir -p "$DEST/usr/share/info"
cp "$WORK/maxima-5.50.0/doc/info"/maxima.info* "$WORK/maxima-5.50.0/doc/info"/maxima-index*.lisp "$DEST/usr/share/info/"
ln -s share/info "$DEST/usr/info"
install -Dm644 "$WORK/maxima-5.50.0/COPYING" "$DEST/usr/share/licenses/maxima/COPYING"
install -Dm644 "$WORK/sbcl-2.5.11-x86-64-linux/COPYING" "$DEST/usr/share/licenses/sbcl/COPYING"
install -Dm644 "$WORK/maxima.tar.gz" "$DEST/usr/share/maximus/sources/maxima-5.50.0.tar.gz"
install -Dm755 "$ROOT/packaging/appimage/maxima" "$DEST/usr/bin/maxima"
test -x "$DEST/usr/lib/maxima/5.50.0/binary-sbcl/maxima"
test -f "$DEST/usr/share/info/maxima.info"
