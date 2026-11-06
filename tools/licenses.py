"""Writes ui/src/licenses.json, the data of Settings > Help > Open source
licenses: the app's own license, the license texts of every Rust crate in the
app (from cargo-about, see about.toml), the npm packages in the UI bundle, and
the engines that the app downloads at first run.

Run it after a dependency changes, and commit the file:

    python -I tools/licenses.py

CI runs it with --check, which fails when the file is out of date.
"""

import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "ui" / "src" / "licenses.json"
UI = ROOT / "ui"

# The engines are not in the installer: the app downloads them from their
# projects, and each package carries its own license files.
ENGINES = [
    {
        "name": "Ollama",
        "license": "MIT",
        "url": "https://github.com/ollama/ollama",
        "note": "The chat engine. Its package also holds the NVIDIA CUDA "
        "libraries, under NVIDIA's license, and the Microsoft Visual C++ "
        "runtime.",
    },
    {
        "name": "stable-diffusion.cpp",
        "license": "MIT",
        "url": "https://github.com/leejet/stable-diffusion.cpp",
        "note": "The picture engine, with ggml (MIT) and libwebp (BSD-3-Clause).",
    },
]


def rust_licenses():
    """cargo-about's licenses for the app crate. Offline, so the output does
    not depend on the network: run `cargo fetch` first."""
    # A file, not stdout: cargo-about refuses a pipe when PowerShell started it.
    with tempfile.TemporaryDirectory() as tmp:
        file = pathlib.Path(tmp) / "about.json"
        done = subprocess.run(
            [
                "cargo", "about", "generate", "--format", "json", "--offline",
                "--manifest-path", str(ROOT / "crates" / "unstrung-app" / "Cargo.toml"),
                "--output-file", str(file),
            ],
            cwd=ROOT, capture_output=True,
        )
        if done.returncode != 0:
            sys.exit("cargo about failed:" + chr(10) + done.stderr.decode("utf-8", "replace"))
        about = json.loads(file.read_text(encoding="utf-8"))
    for lic in about["licenses"]:
        used = sorted({f'{u["crate"]["name"]} {u["crate"]["version"]}' for u in lic["used_by"]})
        # Our own crates are under the app's license, shown first.
        used = [u for u in used if not u.startswith("unstrung-")]
        if used:
            yield lic["name"], lic["text"].strip(), used


# C code that a crate compiles into the app. cargo-about lists the crate's
# own license only, so these texts come from the sources in the crate.
BUNDLED_C = [
    ("libsqlite3-sys", "sqlcipher/LICENSE", "BSD-3-Clause", "SQLCipher (in libsqlite3-sys {v})"),
    ("openssl-src", "openssl/LICENSE.txt", "Apache-2.0", "OpenSSL (built by openssl-src {v})"),
]


def bundled_c_licenses():
    done = subprocess.run(
        [
            "cargo", "metadata", "--format-version", "1", "--offline",
            "--manifest-path", str(ROOT / "crates" / "unstrung-app" / "Cargo.toml"),
        ],
        cwd=ROOT, capture_output=True, check=True,
    )
    packages = json.loads(done.stdout)["packages"]
    for crate, file, name, label in BUNDLED_C:
        found = [p for p in packages if p["name"] == crate]
        if len(found) != 1:
            sys.exit(f"expected one {crate} in the build, found {len(found)}")
        folder = pathlib.Path(found[0]["manifest_path"]).parent
        text = (folder / file).read_text(encoding="utf-8").strip()
        yield name, text, [label.format(v=found[0]["version"])]


def npm_licenses():
    """The packages in the UI bundle: production dependencies only."""
    listed = subprocess.run(
        "npm ls --omit=dev --all --parseable",
        cwd=UI, check=True, capture_output=True, shell=True,
    ).stdout.decode("utf-8").splitlines()
    for line in sorted(set(listed)):
        folder = pathlib.Path(line.strip())
        if folder == UI or not (folder / "package.json").is_file():
            continue
        package = json.loads((folder / "package.json").read_text(encoding="utf-8"))
        files = sorted(
            p for p in folder.iterdir()
            if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE"))
            and not p.name.endswith(".spdx")
        )
        if not files:
            sys.exit(f"{package['name']} has no license file")
        text = "\n\n".join(p.read_text(encoding="utf-8").strip() for p in files)
        yield package.get("license", "see text"), text, [f"{package['name']} {package['version']}"]


def build():
    # Same text, one entry: many crates share a license file.
    merged = {}
    for name, text, used in [*rust_licenses(), *bundled_c_licenses(), *npm_licenses()]:
        entry = merged.setdefault(text, {"name": name, "text": text, "used_by": []})
        entry["used_by"] = sorted(set(entry["used_by"]) | set(used))
    licenses = sorted(merged.values(), key=lambda e: (e["name"], e["used_by"][0]))
    return {
        "app": (ROOT / "LICENSE").read_text(encoding="utf-8").strip(),
        "licenses": licenses,
        "engines": ENGINES,
    }


def main():
    data = json.dumps(build(), indent=1, ensure_ascii=False) + "\n"
    if "--check" in sys.argv:
        if not OUT.is_file() or OUT.read_text(encoding="utf-8") != data:
            sys.exit("ui/src/licenses.json is out of date: run python -I tools/licenses.py")
        print("ui/src/licenses.json is up to date")
        return
    OUT.write_text(data, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(data) // 1024} KB")


if __name__ == "__main__":
    main()
