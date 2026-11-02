"""Validate retained M2 growth datasets and summarize model/VT/process data separately."""
import argparse
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path


def read(path):
    with path.open(encoding="utf-8", newline="") as stream:
        rows = list(csv.DictReader(stream))
    if any(None in row or None in row.values() for row in rows):
        raise ValueError(f"Malformed CSV: {path}")
    return rows


def p95(values):
    values = sorted(values)
    return values[(len(values) * 95 + 99) // 100 - 1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    data = root / "docs/benchmarks"
    output = (args.output_dir or data).resolve()
    output.relative_to(root)
    output.mkdir(parents=True, exist_ok=True)
    paths = [output / f"M2-growth-{name}.csv" for name in
             ("cold-comparison", "native-phase-summary", "native-timing-summary")]
    if any(path.exists() for path in paths):
        raise FileExistsError("Refusing to overwrite retained growth summaries")
    cold = []
    example_hashes = set()
    for pair in (1, 2, 3):
        for version in ("baseline", "current"):
            suffix = "" if pair == 1 else f"-r{pair}"
            prefix = data / f"M2-cold-growth-{version}{suffix}"
            meta = json.loads(Path(str(prefix) + "-metadata.json").read_text(encoding="utf-8"))
            example_hashes.add(meta["example_sha256"])
            rows = read(Path(str(prefix) + "-samples.csv"))
            assert len(rows) == 930 and all(len(row) == 27 for row in rows)
            groups = defaultdict(list)
            identities = set()
            for row in rows:
                identity = (row["corpus_bytes"], row["location"], row["sample"], row["stage"])
                assert identity not in identities
                identities.add(identity)
                assert 0 <= int(row["sample"]) < 31
                groups[(int(row["corpus_bytes"]), row["location"], row["stage"])].append(row)
            assert len(groups) == 30
            for (size, location, stage), group in sorted(groups.items()):
                assert len(group) == 31
                times = [int(row["elapsed_ns"]) for row in group]
                cold.append([pair, version, size, location, stage, len(group),
                             statistics.median(times), p95(times), max(times),
                             statistics.median(int(row["projection_bytes_before"]) for row in group),
                             statistics.median(int(row["projection_bytes_after"]) for row in group),
                             max(int(row["scanned_bytes"]) for row in group),
                             max(int(row["decoded_bytes"]) for row in group)])
    assert len(example_hashes) == 1, "Comparison must use identical benchmark source"
    phases, timings = [], []
    for version in ("before", "after"):
        prefix = data / f"M2-evidence-dense-edits-{version}-native"
        checks = read(Path(str(prefix) + "-checks.csv"))
        assert {row["run"] for row in checks} == {str(run) for run in range(1, 8)}
        assert all([row[key] for key in ("edited_documents", "undone_documents",
                   "cleanly_closed_documents", "disk_unchanged_after_edit",
                   "disk_unchanged_after_undo", "exit_code")] ==
                   ["8", "8", "8", "true", "true", "0"] for row in checks)
        memory = read(Path(str(prefix) + "-memory.csv"))
        groups, missing = defaultdict(list), defaultdict(int)
        for row in memory:
            for role in ("editor", "host", "console", "harness"):
                key = (int(row["run"]), row["phase"], role)
                if int(row[role + "_private"]) == 0:
                    missing[key] += 1  # unavailable/exited process is not zero RAM
                else:
                    groups[key].append(row)
        for run in range(1, 8):
            for phase in ("before_edit", "after_edit", "after_undo", "closed"):
                assert groups[(run, phase, "editor")], (version, run, phase)
        for (run, phase, role), group in sorted(groups.items()):
            private = [int(row[role + "_private"]) for row in group]
            working = [int(row[role + "_working"]) for row in group]
            peak = [int(row[role + "_peak_commit"]) for row in group]
            phases.append([version, run, phase, role, len(group), missing[(run, phase, role)],
                           statistics.median(private), max(private), statistics.median(working),
                           max(working), max(peak)])
        groups = defaultdict(list)
        for row in read(Path(str(prefix) + "-timings.csv")):
            groups[row["operation"]].append(row)
        assert len(groups["startup_ready"]) == 7
        assert len(groups["injection_to_vt"]) == 217
        for operation, group in sorted(groups.items()):
            if operation not in ("startup_ready", "injection_to_vt"):
                assert len(group) == 56
                assert {(row["run"], row["sample"]) for row in group} == {
                    (str(run), str(document)) for run in range(1, 8) for document in range(8)}
            values = [float(row["elapsed_ms"]) for row in group]
            timings.append([version, operation, len(values), statistics.median(values),
                            p95(values), max(values)])
    headers = [
        ["pair", "version", "corpus_bytes", "location", "stage", "samples", "p50_ns",
         "p95_ns", "max_ns", "projection_before_p50", "projection_after_p50",
         "scanned_bytes_max", "decoded_bytes_max"],
        ["version", "run", "phase", "process", "samples", "unavailable_samples",
         "private_p50", "private_max", "working_p50", "working_max", "lifetime_peak_commit"],
        ["version", "operation", "samples", "p50_ms", "p95_ms", "max_ms"],
    ]
    for path, header, rows in zip(paths, headers, (cold, phases, timings)):
        with path.open("x", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream, lineterminator="\n")
            writer.writerow(header)
            writer.writerows(rows)
    print(f"Validated 6 cold captures, 5580 model observations and 14 native retained runs; summaries: {output}")


if __name__ == "__main__":
    main()
