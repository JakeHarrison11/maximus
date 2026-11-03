#!/usr/bin/env python3
"""Stage on-demand Maxima share packages and record runtime provenance.

packages.json schema (version 1):
{
  "schema_version": 1,
  "maxima_version": "<source revision>",
  "manifest_url": "./packages/<maxima_version>/manifest.json",
  "packages": {
    "<package>": {
      "virtual_root": "/maxima/share/<package>",
      "files": [
        {
          "path": "<path relative to virtual_root>",
          "url": "./packages/<maxima_version>/<package>/<path>",
          "size": 123,
          "sha256": "<hex digest>"
        }
      ]
    }
  }
}

The package files are hosted beside the core runtime but are not added to the
eager asset manifest. A worker can fetch only the listed package files needed
by a requested Maxima expression and place them at virtual_root/path.
"""

import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_value(root: pathlib.Path, *args: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "-C", str(root), *args], text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def copy_packages(
    source: pathlib.Path, runtime: pathlib.Path, version: str, names: list[str]
) -> dict:
    version_dir = runtime / "packages" / version
    version_dir.mkdir(parents=True, exist_ok=True)
    packages = {}
    for name in sorted(set(names)):
        source_dir = source / "share" / name
        target_dir = version_dir / name
        target_dir.mkdir(parents=True, exist_ok=True)
        file_entries = []
        for item in sorted(source_dir.rglob("*")):
            if not item.is_file() or item.is_symlink():
                continue
            relative = item.relative_to(source_dir)
            target = target_dir / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(item, target)
            relative_url = relative.as_posix()
            file_entries.append(
                {
                    "path": relative_url,
                    "url": f"./packages/{version}/{name}/{relative_url}",
                    "size": target.stat().st_size,
                    "sha256": sha256(target),
                }
            )
        packages[name] = {
            "virtual_root": f"/maxima/share/{name}",
            "files": file_entries,
        }
    return packages


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime", required=True, type=pathlib.Path)
    parser.add_argument("--maxima-src", required=True, type=pathlib.Path)
    parser.add_argument("--maxima-version", required=True)
    parser.add_argument("--emsdk-version", required=True)
    parser.add_argument("--ecl-version", required=True)
    parser.add_argument("--ecl-wasm", required=True, type=pathlib.Path)
    parser.add_argument("--ecl-host", required=True, type=pathlib.Path)
    parser.add_argument("--ecl-src", required=True, type=pathlib.Path)
    parser.add_argument("--help-info", type=pathlib.Path)
    parser.add_argument("--package", action="append", default=[])
    args = parser.parse_args()

    runtime = args.runtime.resolve()
    maxima_src = args.maxima_src.resolve()
    packages = copy_packages(
        maxima_src, runtime, args.maxima_version, args.package
    )
    package_manifest = {
        "schema_version": 1,
        "maxima_version": args.maxima_version,
        "packages": packages,
    }
    manifest_path = runtime / "packages" / args.maxima_version / "manifest.json"
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(
        json.dumps(package_manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    package_index = {
        "schema_version": 1,
        "maxima_version": args.maxima_version,
        "manifest_url": f"./packages/{args.maxima_version}/manifest.json",
        "packages": packages,
    }
    (runtime / "packages.json").write_text(
        json.dumps(package_index, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    ecl_wasm = args.ecl_wasm.resolve()
    help_info = args.help_info.resolve() if args.help_info else runtime / "help.info"
    repository = pathlib.Path(__file__).resolve().parents[1]
    hash_inputs = {
        "ecl_wasm/libecl.a": ecl_wasm / "libecl.a",
        "ecl_wasm/libeclgc.a": ecl_wasm / "libeclgc.a",
        "ecl_wasm/libeclgmp.a": ecl_wasm / "libeclgmp.a",
        "ecl_source/asdf.lisp": args.ecl_src / "contrib/asdf/asdf.lisp",
        "ecl_wasm/help.doc": ecl_wasm / "help.doc",
        "maxima/COPYING": maxima_src / "COPYING",
        "maxima/help.info": help_info,
        "runtime/embed.c": repository / "crates/maximus-backend-web/runtime/embed.c",
        "runtime/boot.lisp": repository / "crates/maximus-backend-web/runtime/boot.lisp",
        "runtime/driver.lisp": repository / "crates/maximus-backend-native/src/driver.lisp",
        "runtime/load-plan.txt": runtime / "load-plan.txt",
        "build/compile-maxima-wasm.lisp": repository / "scripts/compile-maxima-wasm.lisp",
        "ecl_wasm/target-info.lsp": ecl_wasm / "target-info.lsp",
    }
    checksums = {name: sha256(path) for name, path in hash_inputs.items()}
    emcc = pathlib.Path(shutil.which("emcc") or "")
    if emcc.is_file():
        checksums["emscripten/emcc"] = sha256(emcc)

    provenance = {
        "schema_version": 1,
        "toolchain_baseline": {
            "emscripten": "4.0.15",
            "ecl": "26.3.27",
        },
        "versions": {
            "emscripten": args.emsdk_version,
            "ecl": args.ecl_version,
            "maxima": args.maxima_version,
            "maxima_commit": git_value(maxima_src, "rev-parse", "HEAD"),
            "ecl_source_commit": git_value(
                args.ecl_src, "rev-parse", "HEAD"
            ),
            "node": subprocess.check_output(["node", "--version"], text=True).strip(),
        },
        "source_state": {
            "maxima_dirty": bool(git_value(maxima_src, "status", "--porcelain")),
            "ecl_host": str((args.ecl_host / "bin" / "ecl").resolve()),
        },
        "sha256": checksums,
        "optional_packages": sorted(packages),
    }
    (runtime / "build-info.json").write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
