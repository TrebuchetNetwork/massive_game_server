#!/usr/bin/env python3
"""Fail CI when criterion benchmark medians exceed configured thresholds."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate criterion benchmark medians against threshold budgets."
    )
    parser.add_argument(
        "--thresholds",
        default="scripts/bench_thresholds.json",
        help="Path to threshold manifest JSON",
    )
    parser.add_argument(
        "--criterion-dir",
        default="target/criterion",
        help="Criterion output directory",
    )
    args = parser.parse_args()

    threshold_path = Path(args.thresholds)
    criterion_dir = Path(args.criterion_dir)

    manifest = load_json(threshold_path)
    thresholds = manifest.get("benchmarks", {})
    if not thresholds:
        print(f"[bench-check] no thresholds found in {threshold_path}", file=sys.stderr)
        return 2

    failures: list[str] = []
    print("[bench-check] median thresholds (ns):")
    for bench_name, max_ns in thresholds.items():
        estimates_path = criterion_dir / bench_name / "new" / "estimates.json"
        if not estimates_path.exists():
            failures.append(f"{bench_name}: missing result file {estimates_path}")
            continue
        estimates = load_json(estimates_path)
        median = (
            estimates.get("median", {})
            .get("point_estimate")
        )
        if median is None:
            failures.append(f"{bench_name}: missing median.point_estimate in {estimates_path}")
            continue
        median_ns = float(median)
        ratio = median_ns / float(max_ns) if max_ns else float("inf")
        print(
            f"  - {bench_name}: {median_ns:.2f} ns (limit {max_ns:.2f} ns, ratio {ratio:.2f}x)"
        )
        if median_ns > float(max_ns):
            failures.append(
                f"{bench_name}: median {median_ns:.2f} ns exceeds limit {float(max_ns):.2f} ns"
            )

    if failures:
        print("[bench-check] FAIL:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("[bench-check] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
