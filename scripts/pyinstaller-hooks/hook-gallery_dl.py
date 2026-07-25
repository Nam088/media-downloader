# -*- coding: utf-8 -*-
#
# Vendored verbatim from mikf/gallery-dl's own build tooling
# (scripts/hook-gallery_dl.py, v1.32.8, GPLv2), since gallery-dl only
# publishes official standalone binaries for Windows/Linux (as --onefile,
# which pays a PyInstaller re-extraction cost on every single launch) and
# none at all for macOS. This PyInstaller hook is REQUIRED, not optional:
# gallery-dl imports its ~282 extractor modules dynamically, so without
# these explicit hidden-imports, PyInstaller's static analysis silently
# omits them and the resulting binary can't actually extract from any site.
# See scripts/build-gallery-dl-onedir.sh for how this is used.

from gallery_dl import extractor, downloader, postprocessor
import os

hiddenimports = [
    f"{package.__name__}.{module}"
    for package in (extractor, downloader, postprocessor)
    for module in package.modules
]

base = extractor.__name__ + ".utils."
path = os.path.join(extractor.__path__[0], "utils")
hiddenimports.extend(
    base + file[:-3]
    for file in os.listdir(path)
    if not file.startswith("__")
)

hiddenimports.append("yt_dlp")

mypyc = "81d243bd2c585b0f4821__mypyc"
try:
    from importlib.metadata import files
    for file in files("charset_normalizer"):
        if "__mypyc" in file.name:
            mypyc = file.name.partition(".")[0]
            break
except Exception:
    pass
hiddenimports.append(mypyc)

