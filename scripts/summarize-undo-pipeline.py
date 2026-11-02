"""Summarize paired in-process undo evidence separately from native VT repeats."""
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path


def read(path):
    with path.open(encoding="utf-8", newline="") as stream:
        rows = list(csv.DictReader(stream))
    assert rows and all(None not in row and None not in row.values() for row in rows), path
    return rows


def stats(values):
    values = sorted(values)
    return [len(values), statistics.median(values), values[(len(values) * 95 + 99) // 100 - 1], values[-1]]


def main():
    data = Path(__file__).resolve().parent.parent / "docs/benchmarks"
    outputs = [data / f"M2-undo-{name}-summary.csv" for name in ("pipeline", "native-repeat")]
    if any(path.exists() for path in outputs):
        raise FileExistsError("Refusing to overwrite retained summaries")
    pipeline, native, hashes = [], [], set()
    for pair in (1, 2):
        for version in ("flat", "block"):
            prefix = data / f"M2-undo-pipeline-{version}-r{pair}"
            meta = json.loads(Path(str(prefix) + "-metadata.json").read_text(encoding="utf-8"))
            hashes.add(meta["example_sha256"])
            rows = read(Path(str(prefix) + "-samples.csv"))
            assert len(rows) == 496 and all(len(row) == 11 and row["full"] == "false" for row in rows)
            assert {(int(row["document"]), row["operation"], int(row["sample"])) for row in rows} == {
                (doc, op, sample) for doc in range(8) for op in ("enter", "undo") for sample in range(31)}
            assert "checks passed" in Path(str(prefix) + "-checks.log").read_text(encoding="utf-8")
            for document in (*map(str, range(8)), "all"):
                for operation in ("enter", "undo"):
                    group = [row for row in rows if row["operation"] == operation
                             and (document == "all" or row["document"] == document)]
                    for stage in ("handle_event_ns", "frame_ns", "encode_ns", "total_ns"):
                        pipeline.append([pair, version, document, operation, stage,
                                         *stats([int(row[stage]) for row in group])])
    assert len(hashes) == 1, "Both versions must use the identical probe"
    for version in ("flat", "block"):
        prefix = data / f"M2-evidence-dense-edits-{version}-repeat-native"
        checks = read(Path(str(prefix) + "-checks.csv"))
        assert len(checks) == 7 and {row["run"] for row in checks} == set(map(str, range(1, 8)))
        for row in checks:
            assert [row[key] for key in ("edited_documents", "undone_documents", "cleanly_closed_documents",
                    "disk_unchanged_after_edit", "disk_unchanged_after_undo", "exit_code")] == ["8", "8", "8", "true", "true", "0"]
        groups = defaultdict(list)
        for row in read(Path(str(prefix) + "-timings.csv")):
            groups[row["operation"]].append(row)
        assert len(groups["startup_ready"]) == 7 and len(groups["injection_to_vt"]) == 217
        for operation, group in sorted(groups.items()):
            if operation not in ("startup_ready", "injection_to_vt"):
                assert len(group) == 56 and {(int(row["run"]), int(row["sample"])) for row in group} == {
                    (run, doc) for run in range(1, 8) for doc in range(8)}
            native.append([version, operation, *stats([float(row["elapsed_ms"]) for row in group])])
        memory = read(Path(str(prefix) + "-memory.csv"))
        print(version, "native peak commit MiB", max(int(row["editor_peak_commit"]) for row in memory) / 1048576)
    headers = [
        ["pair", "version", "document", "operation", "stage", "samples", "p50_ns", "p95_ns", "max_ns"],
        ["version", "operation", "samples", "p50_ms", "p95_ms", "max_ms"],
    ]
    for path, header, rows in zip(outputs, headers, (pipeline, native)):
        with path.open("x", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream, lineterminator="\n")
            writer.writerow(header)
            writer.writerows(rows)
    print("Validated 1984 pipeline observations and 14 retained native processes")


if __name__ == "__main__":
    main()
