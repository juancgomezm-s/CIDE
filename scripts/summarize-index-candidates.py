"""Summarize complete metadata experiments; logical capacity is not process RAM."""
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path


def main():
    root = Path(__file__).resolve().parent.parent
    output = root / "docs/benchmarks/M2-index-candidates-summary.csv"
    if output.exists():
        raise FileExistsError("Refusing to overwrite candidate summary")
    hashes, summaries = set(), []
    cases = [(mib, stride) for mib in (1, 4, 8) for stride in (8, 64)] + [(8, 8192), (8, 65536)]
    for pair in (1, 2):
        for mib, stride in cases:
            prefix = root / f"docs/benchmarks/M2-index-candidates-r{pair}-{mib}-{stride}"
            meta = json.loads(Path(str(prefix) + "-metadata.json").read_text(encoding="utf-8"))
            hashes.add((meta["binary_sha256"], json.dumps(meta["source_sha256"], sort_keys=True)))
            assert meta["order"] == ("flat-first" if pair == 1 else "block-first")
            assert "metadata oracles passed: flat + block" in Path(str(prefix) + "-checks.log").read_text(encoding="utf-8")
            with Path(str(prefix) + "-samples.csv").open(encoding="utf-8", newline="") as stream:
                rows = list(csv.DictReader(stream))
            assert len(rows) == 938 and all(len(row) == 10 and None not in row for row in rows)
            groups = defaultdict(list)
            for row in rows:
                assert int(row["source_bytes"]) == mib * 1048576 and int(row["stride"]) == stride
                expected_lines = mib * 1048576 // stride + 1
                if row["operation"].startswith("newline_") and row["operation"].endswith("_forward"):
                    expected_lines += 1
                assert int(row["lines"]) == expected_lines
                groups[(row["implementation"], row["operation"], int(row["batch_size"]))].append(row)
            assert len(groups) == 38
            for (implementation, operation, batch), group in sorted(groups.items()):
                assert implementation in ("flat", "block")
                assert len(group) == (31 if batch else 1)
                if batch:
                    assert {int(row["sample"]) for row in group} == set(range(31))
                    values = sorted(int(row["elapsed_ns"]) for row in group)
                    p50, p95 = statistics.median(values), values[29]
                else:
                    p50 = p95 = ""
                summaries.append([pair, mib * 1048576, stride, implementation, operation,
                                  len(group), batch, p50, p95,
                                  p95 / batch if batch == 4096 else "",
                                  min(int(row["logical_bytes"]) for row in group),
                                  max(int(row["logical_bytes"]) for row in group)])
    assert len(hashes) == 1, "All captures must use the same sources/binary"
    with output.open("x", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(["pair", "source_bytes", "stride", "implementation", "operation",
                         "samples", "batch_size", "p50_batch_ns", "p95_batch_ns",
                         "p95_batch_mean_per_lookup_ns", "logical_bytes_min", "logical_bytes_max"])
        writer.writerows(summaries)
    print("Validated 16 complete captures / 15008 rows; summary saved. Derived batch means are not individual lookup p95.")


if __name__ == "__main__":
    main()
