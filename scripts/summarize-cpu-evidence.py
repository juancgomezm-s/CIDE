"""Validate identical CPU workloads and summarize raw samples, never sums of quantiles."""
import argparse
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


def quantile(values, percent):
    values = sorted(values)
    return values[(len(values) * percent + 99) // 100 - 1]


def summarize(prefix, expected_cases, expected_samples):
    rows = read(Path(str(prefix) + "-samples.csv"))
    groups = defaultdict(list)
    for row in rows:
        if row["kind"] == "sample":
            groups[(row["case"], row["encoding"], int(row["corpus_bytes"]))].append(row)
    assert len(groups) == expected_cases
    output = {}
    for key, group in sorted(groups.items()):
        assert len(group) == expected_samples
        assert {int(row["sample"]) for row in group} == set(range(expected_samples))
        identity_keys = ("protocol", "files", "corpus_hash", "batch", "concurrent_load",
                         "state", "rows", "issues", "checksum", "throughput_basis")
        identities = {tuple(row[column] for column in identity_keys) for row in group}
        assert len(identities) == 1
        wall = [float(row["wall_ns"]) for row in group]
        # CPU is summed for each original operation before taking a percentile.
        cpu = [float(row["user_ns"]) + float(row["kernel_ns"]) for row in group]
        throughput = [float(row["throughput_from_wall_mib_s"]) for row in group
                      if row["throughput_from_wall_mib_s"]]
        output[key] = {
            "identity": next(iter(identities)),
            "values": [len(group), statistics.median(wall), quantile(wall, 95),
                       statistics.median(cpu), quantile(cpu, 95), sum(cpu), sum(wall),
                       sum(cpu) / sum(wall),
                       statistics.median(throughput) if throughput else "",
                       max(float(row["private_peak_observed"]) for row in group),
                       max(float(row["working_peak_observed"]) for row in group),
                       max(float(row["lifetime_peak_pagefile"]) for row in group)],
        }
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--codegen-only", action="store_true")
    args = parser.parse_args()
    data = Path(__file__).resolve().parent.parent / "docs/benchmarks"
    if args.codegen_only:
        path = data / "M2-cpu-codegen-summary.csv"
        if path.exists():
            raise FileExistsError(path)
        initial = json.loads((data / "M2-cpu-profile-serial-r1-metadata.json").read_text())
        rows = []
        identities = {}
        for pair in (1, 2):
            for level in ("s", "2", "3"):
                prefix = data / f"M2-cpu-codegen-{level}-r{pair}"
                meta = json.loads(Path(str(prefix) + "-metadata.json").read_text())
                assert meta["example_sha256"] == initial["example_sha256"]
                for key, result in summarize(prefix, 4, 7).items():
                    assert key not in identities or identities[key] == result["identity"]
                    identities[key] = result["identity"]
                    values = result["values"]
                    rows.append([pair, level, meta["binary_bytes"], *key,
                                 values[0], values[1], values[2], values[3],
                                 values[5], values[7], values[9]])
        with path.open("x", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream, lineterminator="\n")
            writer.writerow(["pair", "app_opt_level", "probe_binary_bytes", "case",
                             "encoding", "corpus_bytes", "samples", "wall_p50_ns",
                             "wall_p95_ns", "cpu_total_p50_ns", "cpu_total_sum_ns",
                             "aggregate_equivalent_cores", "private_observed_peak_bytes"])
            writer.writerows(rows)
        print("Validated 6 package-codegen captures;168 retained samples; selection only")
        return
    paths = [data / f"M2-cpu-{name}.csv" for name in
             ("profile-summary", "search-summary", "search-comparison")]
    if any(path.exists() for path in paths):
        raise FileExistsError("Refusing to overwrite retained CPU summaries")
    initial = data / "M2-cpu-profile-serial-r1"
    meta = json.loads(Path(str(initial) + "-metadata.json").read_text())
    profile = summarize(initial, 18, 7)
    initial_rows = [[*key, *result["values"]] for key, result in profile.items()]
    summaries, comparisons = [], []
    for pair in (1, 2):
        versions = {}
        for version in ("serial", "parallel"):
            prefix = data / f"M2-cpu-search-{version}-r{pair}"
            identity = json.loads(Path(str(prefix) + "-metadata.json").read_text())
            assert identity["example_sha256"] == meta["example_sha256"]
            versions[version] = summarize(prefix, 12, 31)
            for key, result in versions[version].items():
                assert result["identity"] == profile[key]["identity"]
                assert result["identity"][5] == "Complete" and result["identity"][7] == "0"
                summaries.append([pair, version, *key, *result["values"]])
        assert versions["serial"].keys() == versions["parallel"].keys()
        for key in versions["serial"]:
            before = versions["serial"][key]["values"]
            after = versions["parallel"][key]["values"]
            comparisons.append([pair, *key, before[1] / after[1], before[2] / after[2],
                                after[5] / before[5], before[7], after[7],
                                after[9] - before[9]])
    metric_header = ["samples", "wall_p50_ns", "wall_p95_ns", "cpu_total_p50_ns",
                     "cpu_total_p95_ns", "cpu_total_sum_ns", "wall_sum_ns",
                     "aggregate_equivalent_cores", "throughput_p50_mib_s",
                     "private_observed_peak_bytes", "working_observed_peak_bytes",
                     "lifetime_peak_pagefile_bytes"]
    case_header = ["case", "encoding", "corpus_bytes"]
    headers = [case_header + metric_header, ["pair", "version"] + case_header + metric_header,
               ["pair"] + case_header + ["wall_p50_speedup", "wall_p95_speedup",
                 "cpu_total_ratio_after_before", "cores_before", "cores_after",
                 "private_observed_peak_delta_bytes"]]
    for path, header, rows in zip(paths, headers, (initial_rows, summaries, comparisons)):
        with path.open("x", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream, lineterminator="\n")
            writer.writerow(header)
            writer.writerows(rows)
    print("Validated 18 initial profiles and 4 paired captures; 1488 retained search samples")


if __name__ == "__main__":
    main()
