"""Writes crates/unstrung-core/catalog/engine-files.json, the SHA-256 of every
file in each pinned engine package, and prints the unpacked size of each
package for catalog/engines.json. Full repair and the install check compare
the unpacked engine with this list.

Run it after you change the engine version in catalog/engines.json, with the
downloaded zips that match the hashes in that file:

    python -I tools/engine_files.py base=ollama-windows-amd64.zip amd=ollama-windows-amd64-rocm.zip
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
    engines = json.loads((CATALOG / "engines.json").read_text(encoding="utf-8"))
    packages = {p["id"]: p for p in engines["ollama"]["packages"]}
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
    target = CATALOG / "engine-files.json"
    target.write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {target}")


if __name__ == "__main__":
    main(sys.argv[1:])
