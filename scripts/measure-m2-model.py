"""Run an already-built opt-in benchmark, one fresh process per corpus. No downloads."""
import argparse
import csv
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument("phase", choices=("before", "after"))
args = parser.parse_args()
env = os.environ.copy()
temp = root / "work/test-temp"
temp.mkdir(parents=True, exist_ok=True)
env.update(TEMP=str(temp), TMP=str(temp))
output = root / "docs/benchmarks" / ("M2-evidence-" + args.phase + "-model.csv")
if output.exists():
    raise SystemExit("Refusing to overwrite measurement: " + str(output))
rows = []
for case in ("ascii", "unicode", "cp1252", "latin1", "ibm850", "long", "pieces", "multi"):
    run = subprocess.run([str(root / "target/release/examples/m2_acceptance.exe"), case],
                         cwd=root, env=env, capture_output=True, text=True, check=True)
    rows.extend(csv.DictReader(run.stdout.splitlines()))
    print(case, "complete", flush=True)
with output.open("w", encoding="utf-8", newline="") as file:
    writer = csv.DictWriter(file, fieldnames=rows[0].keys(), lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
print(output)
