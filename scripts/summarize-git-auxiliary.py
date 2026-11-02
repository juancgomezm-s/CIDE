"""Summarize simultaneous observed editor/descendant samples, not summed peaks."""
import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("prefix", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    prefix = args.prefix.resolve()
    prefix.relative_to(root)
    output = Path(str(prefix) + "-summary.csv")
    if output.exists():
        raise FileExistsError(f"Refusing to overwrite {output}")
    metadata = json.loads(Path(str(prefix) + "-metadata.json").read_text(encoding="utf-8"))
    assert metadata["requested_sample_ms"] == 5
    with Path(str(prefix) + "-runs.csv").open(encoding="utf-8", newline="") as stream:
        runs = list(csv.DictReader(stream))
    assert len(runs) == 9
    checks = ("head_async", "index_async", "hunk_navigation", "stale_after_undo",
              "sidebar_survives_hidden_bottom", "disk_index_unchanged")
    for run in runs:
        assert all(run[key].lower() == "true" for key in checks), run
        assert run["exit_code"] == "0"
        assert run["truncated"].lower() == run["sampling_failed"].lower() == "false"
    samples = defaultdict(list)
    with Path(str(prefix) + "-processes.csv").open(encoding="utf-8", newline="") as stream:
        for row in csv.DictReader(stream):
            samples[(int(row["fixture_bytes"]), int(row["run"]), int(row["sample"]))].append(row)
    groups = defaultdict(list)
    for (size, run, sample), rows in samples.items():
        editor = [row for row in rows if row["role"] == "editor"]
        assert len(editor) == 1, (size, run, sample)
        children = [row for row in rows if row["role"] != "editor"]
        groups[(size, run, editor[0]["phase"])].append({
            "elapsed": float(editor[0]["elapsed_ms"]),
            "editor": int(editor[0]["private_bytes"]),
            "children": sum(int(row["private_bytes"]) for row in children),
            "working": sum(int(row["working_set_bytes"]) for row in children),
            "identities": {(row["pid"], row["start_ticks"]) for row in children},
        })
    header = ["fixture_bytes", "run", "phase", "samples", "observed_editor_private_peak",
              "observed_children_private_peak", "observed_combined_private_peak",
              "observed_children_working_peak", "observed_children_max_live",
              "unique_descendants_observed", "max_sample_gap_ms"]
    summaries = []
    for (size, run, phase), rows in sorted(groups.items()):
        rows.sort(key=lambda row: row["elapsed"])
        identities = set().union(*(row["identities"] for row in rows))
        summaries.append([size, run, phase, len(rows), max(row["editor"] for row in rows),
                          max(row["children"] for row in rows),
                          max(row["editor"] + row["children"] for row in rows),
                          max(row["working"] for row in rows),
                          max(len(row["identities"]) for row in rows), len(identities),
                          f"{max((b['elapsed'] - a['elapsed'] for a, b in zip(rows, rows[1:])), default=0):.4f}"])
    with output.open("x", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(header)
        writer.writerows(summaries)
    for size in sorted({row[0] for row in summaries}):
        peak = max(row[6] for row in summaries if row[0] == size)
        print(f"{size} bytes: observed editor+descendant private peak {peak / 1048576:.3f} MiB")
    print("Sampled observations; short-lived processes and true peaks may be missed.")


if __name__ == "__main__":
    main()
