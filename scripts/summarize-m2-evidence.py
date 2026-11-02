"""Summarize the retained raw local samples, keeping timings and process memory separate."""
import csv
import math
from collections import defaultdict
from pathlib import Path

root = Path(__file__).resolve().parent.parent
folder = root / "docs/benchmarks"
def read(name):
    with (folder / name).open(encoding="utf-8", newline="") as file:
        return list(csv.DictReader(file))
def p(values, percentile):
    values = sorted(values)
    return values[math.ceil(len(values) * percentile) - 1]
for phase in ("before", "after"):
    path = folder / f"M2-evidence-{phase}-model.csv"
    if not path.exists():
        continue
    groups = defaultdict(list)
    for row in read(path.name):
        groups[row["case"], row["operation"]].append(row)
    print(phase, "model: case/op n p50_us p95_us max_peak_live private_at_end process_peak_commit")
    summary = []
    for (case, op), rows in groups.items():
        times = [int(row["elapsed_ns"]) / 1000 for row in rows]
        entry = dict(phase=phase, case=case, operation=op, samples=len(rows),
                     p50_us=p(times, .5), p95_us=p(times, .95),
                     allocation_calls_p95=p([int(r["allocation_calls"]) for r in rows], .95),
                     allocated_bytes_p95=p([int(r["allocated_bytes"]) for r in rows], .95),
                     max_peak_live_bytes=max(int(r["peak_live_bytes"]) for r in rows),
                     last_live_bytes=int(rows[-1]["live_bytes"]),
                     last_private_bytes=int(rows[-1]["private_bytes"]),
                     process_peak_commit_bytes=max(int(r["process_peak_commit_bytes"]) for r in rows))
        summary.append(entry)
        if op in ("cold_projection", "warm_move_viewport", "close_all"):
            print(case, op, len(rows), entry["p50_us"], entry["p95_us"], entry["max_peak_live_bytes"], entry["last_private_bytes"], entry["process_peak_commit_bytes"])
    with (folder / f"M2-evidence-{phase}-model-summary.csv").open("w", encoding="utf-8", newline="") as file:
        writer = csv.DictWriter(file, summary[0].keys(), lineterminator="\n")
        writer.writeheader(); writer.writerows(summary)
    groups = defaultdict(list)
    for row in read(f"M2-evidence-{phase}-native-timings.csv"):
        groups[row["shape"], row["operation"]].append(float(row["elapsed_ms"]))
    for key, values in groups.items():
        print(phase, key, len(values), "p50/p95 ms", p(values, .5), p(values, .95))
    groups = defaultdict(list)
    for row in read(f"M2-evidence-{phase}-native-memory.csv"):
        groups[row["shape"]].append(row)
    for shape, rows in groups.items():
        print(phase, shape, "native memory", len(rows), "samples")
        for role in ("editor", "host", "console", "harness"):
            print(role, "max sampled private", max(int(r[role+"_private"]) for r in rows),
                  "peak commit", max(int(r[role+"_peak_commit"]) for r in rows))
        print("max simultaneous editor+host+console", max(sum(int(r[role+"_private"]) for role in ("editor", "host", "console")) for r in rows))
        for stage in ("ready", "retained", "closed"):
            values = [int(r["editor_private"]) for r in rows if r["phase"] == stage]
            print(stage, "private p50/max", p(values, .5), max(values))
