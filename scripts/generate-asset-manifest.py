#!/usr/bin/env python3
"""Write the eager/offline core asset list after Trunk finishes."""

import argparse
import hashlib
import json
import pathlib


def digest(path: pathlib.Path) -> str:
    sha = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            sha.update(chunk)
    return sha.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", required=True, type=pathlib.Path)
    args = parser.parse_args()
    root = args.dist.resolve()
    required = [
        "index.html",
        "runtime/maxima.mjs",
        "runtime/maxima.data",
        "runtime/maxima.wasm",
        "runtime/help.info",
        "runtime/packages.json",
    ]
    missing = [name for name in required if not (root / name).is_file()]
    if missing:
        raise SystemExit("Required web output is missing: " + ", ".join(missing))

    worker_path = root / "service-worker.js"
    if not worker_path.is_file():
        raise SystemExit(f"Required web output is missing: {worker_path.relative_to(root)}")
    worker_template = worker_path.read_bytes()
    token = b"__MAXIMUS_BUILD_ID__"
    if worker_template.count(token) != 1:
        raise SystemExit("service-worker.js must contain exactly one build ID placeholder")

    assets = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        relative = path.relative_to(root).as_posix()
        if (
            relative in {"asset-manifest.json", "service-worker.js"}
            or relative.startswith("runtime/packages/")
        ):
            continue
        assets.append(
            {
                "url": f"./{relative}",
                "size": path.stat().st_size,
                "sha256": digest(path),
            }
        )

    canonical = json.dumps(
        {
            "files": assets,
            "service_worker_template_sha256": hashlib.sha256(worker_template).hexdigest(),
        },
        separators=(",", ":"),
        sort_keys=True,
    ).encode()
    build_id = hashlib.sha256(canonical).hexdigest()[:20]
    worker = worker_template.replace(token, build_id.encode("ascii"))
    worker_path.write_bytes(worker)
    assets.append(
        {
            "url": "./service-worker.js",
            "size": len(worker),
            "sha256": hashlib.sha256(worker).hexdigest(),
        }
    )
    assets.sort(key=lambda asset: asset["url"])
    manifest = {
        "schema_version": 1,
        "build_id": build_id,
        "files": assets,
        "optional_package_assets_excluded": True,
    }
    (root / "asset-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(f"Manifest {build_id}: {len(assets)} core assets (optional packages excluded)")


if __name__ == "__main__":
    main()
