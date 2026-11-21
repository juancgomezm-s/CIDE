"""Complete a local GUI delivery with the validated, self-contained OmniCore runtime."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import uuid

ROOT = Path(__file__).resolve().parents[1]
PACK = runpy.run_path(str(ROOT / "scripts/package-cide.py"))

def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def complete(delivery, runtime):
    delivery = PACK["checked"](delivery, ROOT, directory=True)
    target = (ROOT / "target").resolve()
    if not delivery.resolve().is_relative_to(target) or delivery.resolve().is_relative_to(target / "build-cache"):
        raise ValueError("Use una entrega propia dentro de target, fuera de la caché")
    exe = PACK["checked"](delivery / "cide-gui.exe", ROOT)
    manifest = PACK["checked"](delivery / "build-manifest.json", ROOT)
    metadata = json.loads(manifest.read_text(encoding="utf-8"))
    if metadata.get("files", {}).get("cide-gui.exe") != digest(exe):
        raise ValueError("El ejecutable GUI no coincide con su manifiesto")
    # Reuse MSI packaging admission: required managed/native components, edit
    # bridge, licenses, reparse rejection and bounded package inventory.
    sources = PACK["omnicore_sources"](ROOT, runtime)
    inventory = {name.removeprefix("bin/omnicore/"): {"bytes":path.stat().st_size,"sha256":digest(path)} for path,name in sources}
    destination = delivery / "omnicore"
    staged = None
    if not destination.exists():
        staged = delivery / (".omnicore-" + uuid.uuid4().hex)
        staged.mkdir()
        for source, name in sources:
            relative = name.removeprefix("bin/omnicore/")
            output = staged / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            with source.open("rb") as incoming, output.open("xb") as outgoing:
                copied = 0
                while chunk := incoming.read(64*1024):
                    copied += len(chunk)
                    if copied > inventory[relative]["bytes"]:
                        raise ValueError("El runtime cambió durante la copia: " + relative)
                    outgoing.write(chunk)
                outgoing.flush()
                os.fsync(outgoing.fileno())
            if output.stat().st_size != inventory[relative]["bytes"] or digest(output) != inventory[relative]["sha256"]:
                raise ValueError("La copia de OmniCore no coincide: " + relative)
        staged.rename(destination)
    actual = PACK["omnicore_sources"](ROOT, destination)
    observed = {name.removeprefix("bin/omnicore/"): {"bytes":path.stat().st_size,"sha256":digest(path)} for path,name in actual}
    if observed != inventory:
        raise ValueError("El runtime existente difiere; no se sobrescribió")
    metadata.setdefault("companions", {})["omnicore"] = {"files": inventory,"bytes":sum(row["bytes"] for row in inventory.values())}
    temporary = manifest.with_name(".manifest-" + uuid.uuid4().hex + ".json")
    with temporary.open("x", encoding="utf-8") as output:
        json.dump(metadata, output, indent=2); output.write("\n"); output.flush(); os.fsync(output.fileno())
    os.replace(temporary, manifest)
    return {"ok":True,"delivery":str(delivery),"runtime_files":len(inventory),"runtime_bytes":sum(row["bytes"] for row in inventory.values()),"gui_sha256":digest(exe)}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--delivery",required=True,type=Path)
    parser.add_argument("--runtime",required=True,type=Path)
    args=parser.parse_args()
    try:
        print(json.dumps(complete(args.delivery.absolute(),args.runtime.absolute())))
    except (OSError,ValueError,PACK["PackagingError"]) as error:
        parser.exit(1,str(error)+"\n")
if __name__ == "__main__":main()
