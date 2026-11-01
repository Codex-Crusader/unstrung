"""Writes one engine's section of crates/unstrung-core/catalog/engine-files.json,
the SHA-256 of every file in each pinned package, and prints the unpacked size
of each package for catalog/engines.json. Full repair and the install check
compare the unpacked engine with this list. Other engines' sections stay.

Run it after you change an engine version in catalog/engines.json, with the
downloaded zips that match the hashes in that file:

    python -I tools/engine_files.py ollama base=ollama-windows-amd64.zip amd=ollama-windows-amd64-rocm.zip
    python -I tools/engine_files.py sdcpp vulkan=sd-...-win-vulkan-x64.zip cpu=sd-...-win-cpu-x64.zip
"""

import hashlib
import json
import pathlib
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATALOG = ROOT / "crates" / "unstrung-core" / "catalog"


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main(args):
    engine, args = args[0], args[1:]
    engines = json.loads((CATALOG / "engines.json").read_text(encoding="utf-8"))
    packages = {p["id"]: p for p in engines[engine]["packages"]}
    target = CATALOG / "engine-files.json"
    all_files = json.loads(target.read_text(encoding="utf-8")) if target.exists() else {}
    out = {}
    for arg in args:
        package_id, _, zip_path = arg.partition("=")
        package = packages[package_id]
        actual = sha256_file(zip_path)
        if actual != package["sha256"]:
            sys.exit(f"{zip_path}: SHA-256 {actual}, but engines.json says {package['sha256']}")
        files = {}
        unpacked = 0
        with zipfile.ZipFile(zip_path) as z:
            for info in z.infolist():
                if info.is_dir():
                    continue
                name = info.filename.replace("\\", "/")
                if name.startswith("/") or ".." in name.split("/"):
                    sys.exit(f"{zip_path}: unsafe name {name}")
                h = hashlib.sha256()
                with z.open(info) as f:
                    for block in iter(lambda: f.read(1 << 20), b""):
                        h.update(block)
                files[name] = h.hexdigest()
                unpacked += info.file_size
        out[package_id] = dict(sorted(files.items()))
        print(f"{package_id}: {len(files)} files, unpacked_size {unpacked}")
    all_files[engine] = out
    target.write_text(json.dumps(all_files, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {target}")


if __name__ == "__main__":
    main(sys.argv[1:])
