"""Build CIDE's edit bridge beside a compatible, self-contained OmniCore runtime."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[1]
BRIDGE = "cide-omni-turn-v2"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", type=Path, help="Local OmniCore runtime directory")
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    if not runtime.is_relative_to(ROOT):
        parser.error("Usa una copia de OmniCore dentro de CIDE.")

    framework = (runtime / "omni.runtimeconfig.json").read_bytes()
    deps = json.loads((runtime / "omni.deps.json").read_text(encoding="utf-8-sig"))
    target = deps["runtimeTarget"]["name"]
    if not target.endswith("/win-x64"):
        parser.error("Se requiere el runtime de OmniCore para Windows x64.")
    original, = [key for key in deps["targets"][target] if key.split("/")[0] == "omni"]
    for libraries in deps["targets"].values():
        if original in libraries:
            entry = libraries.pop(original)
            entry["runtime"] = {f"{BRIDGE}.dll": {}}
            libraries[f"{BRIDGE}/1.0.0"] = entry
    deps["libraries"][f"{BRIDGE}/1.0.0"] = deps["libraries"].pop(original)

    cache = ROOT / "work/cide-omni-turn-build"
    output = cache / "output"
    result = subprocess.run(
        ["dotnet", "build", str(ROOT / "build_support/omnicore-turn/OmniCoreTurn.csproj"),
         "-c", "Release", "-o", str(output), f"-p:OmniCoreRuntime={runtime}",
         f"-p:BaseIntermediateOutputPath={cache / 'obj'}{os.sep}",
         "-p:NuGetAudit=false", "--ignore-failed-sources"],
        cwd=ROOT, check=False,
        env={**os.environ, "DOTNET_CLI_TELEMETRY_OPTOUT": "1"},
    )
    if result.returncode:
        return result.returncode
    (runtime / f"{BRIDGE}.runtimeconfig.json").write_bytes(framework)
    (runtime / f"{BRIDGE}.deps.json").write_text(json.dumps(deps), encoding="utf-8")
    for suffix in (".dll", ".exe"):
        shutil.copy2(output / (BRIDGE + suffix), runtime / (BRIDGE + suffix))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
