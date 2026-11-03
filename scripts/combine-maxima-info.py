#!/usr/bin/env python3
"""Combine plain or gzip-compressed English Maxima Info chunks."""

import gzip
import pathlib
import re
import shutil
import sys


def combine(source: pathlib.Path, target: pathlib.Path) -> None:
    parts = {}
    for path in source.glob("maxima.info-[0-9]*"):
        match = re.fullmatch(r"maxima\.info-([0-9]+)(\.gz)?", path.name)
        if not match or not path.is_file():
            continue
        number = int(match.group(1))
        if number in parts:
            raise ValueError(f"Duplicate Maxima Info chunk {number}: {parts[number]} and {path}")
        parts[number] = path
    if not parts:
        raise ValueError(f"No split English Maxima Info chunks found in {source}")
    with target.open("wb") as out:
        for number in sorted(parts):
            path = parts[number]
            opener = gzip.open if path.suffix == ".gz" else open
            with opener(path, "rb") as stream:
                shutil.copyfileobj(stream, out)


if __name__ == "__main__":
    combine(*map(pathlib.Path, sys.argv[1:]))
