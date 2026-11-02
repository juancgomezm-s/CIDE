"""Compare retained pre-change and final executables in a stable, serial window."""
import argparse
import csv
import hashlib
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("phase", choices=("before", "after"))
args = parser.parse_args()
binary = root / ("work/m2_acceptance-before-dense.exe" if args.phase == "before" else "target/release/examples/m2_acceptance.exe")
output = root / f"docs/benchmarks/M2-dense-{args.phase}-model.csv"
manifest = root / f"docs/benchmarks/M2-dense-{args.phase}-binaries.csv"
if output.exists() or manifest.exists():
    raise SystemExit("Refusing to overwrite retained measurement")
env = os.environ.copy()
env.update(TEMP=str(root / "work/test-temp"), TMP=str(root / "work/test-temp"))
rows = []
for case in ("ascii", "unicode", "cp1252", "latin1", "ibm850", "long", "pieces", "multi"):
    run = subprocess.run([str(binary), case], cwd=root, env=env, capture_output=True, text=True, check=True)
    rows.extend(csv.DictReader(run.stdout.splitlines()))
    print(args.phase, case, "complete", flush=True)
with output.open("w", encoding="utf-8", newline="") as file:
    writer = csv.DictWriter(file, fieldnames=rows[0].keys(), lineterminator="\n")
    writer.writeheader(); writer.writerows(rows)
tui = root / ("work/jc-ide-tui-before-dense.exe" if args.phase == "before" else "target/release/jc-ide-tui.exe")
with manifest.open("w", encoding="utf-8", newline="") as file:
    writer = csv.writer(file, lineterminator="\n"); writer.writerow(("role", "bytes", "sha256"))
    for role, path in (("instrumented_model", binary), ("native_tui", tui)):
        data = path.read_bytes(); writer.writerow((role, len(data), hashlib.sha256(data).hexdigest()))
print(output)
