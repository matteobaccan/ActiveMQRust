#!/usr/bin/env python3
# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# ActiveMQRust -- the downloads grid at the top of a GitHub Release body.
#
# Usage: downloads_table.py <asset dir> <tag> <owner/repo>
# Prints a Markdown table, one row per architecture and one column per system, linking
# every asset found in <asset dir> by its file name. Assets that do not exist yet (a
# package not built for this release) simply leave their cell empty, so the grid follows
# the release pipeline as it grows.

import os
import re
import sys

# (row, column, label, pattern); patterns match the asset names build.yml produces.
ASSETS = [
    ("x86-64", "Windows", "ZIP", r"^activemq-rust-windows-x86_64-[0-9].*\.zip$"),
    ("ARM64", "macOS", "tar.gz", r"^activemq-rust-macos-arm64-[0-9].*\.tar\.gz$"),
]
ROWS = [("x86-64", "**x86-64** (64-bit)"), ("ARM64", "**ARM64** (Apple Silicon)")]
COLUMNS = ["Windows", "macOS"]


def main() -> None:
    folder, tag, repo = sys.argv[1:4]
    names = sorted(os.listdir(folder))
    base = f"https://github.com/{repo}/releases/download/{tag}"
    cells = {}
    for row, col, label, pattern in ASSETS:
        for name in names:
            if re.match(pattern, name):
                cells.setdefault((row, col), []).append(f"[{label}]({base}/{name})")
    print("## Downloads")
    print()
    print("| Architecture | " + " | ".join(COLUMNS) + " |")
    print("|---|" + "---|" * len(COLUMNS))
    for key, title in ROWS:
        line = [" ".join(cells.get((key, col), [])) or "—" for col in COLUMNS]
        print(f"| {title} | " + " | ".join(line) + " |")
    print()
    symbols = [n for n in names if "-symbols-" in n]
    notes = [
        "Every download holds the broker (`mqrust.exe` on Windows, `mqrust` on macOS), LICENSE, "
        "README.md and the commented configuration template `mqrust.example.toml`. "
        "The Windows service commands are available on Windows only."
    ]
    if symbols:
        links = ", ".join(f"[{n}]({base}/{n})" for n in symbols)
        notes.append(f"Debug symbols, only needed to read a crash dump: {links}.")
    notes.append(
        "The builds are not signed: Windows SmartScreen asks to confirm (More info → Run "
        "anyway); on macOS remove the quarantine flag once with "
        "`xattr -d com.apple.quarantine mqrust` (or System Settings → Privacy & Security → "
        "Open anyway)."
    )
    for n in notes:
        print(n)
        print()


if __name__ == "__main__":
    main()
