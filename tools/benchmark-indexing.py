"""Read-only native indexing benchmark; use a fixed cache snapshot for comparisons.

No GUI is opened and no index is reused or modified. Model files must already exist.
Detailed output (including asset IDs) stays in memory; only timings/counts are printed.
"""
import argparse
import json
import statistics
import subprocess
import time
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--exe", type=Path, required=True)
parser.add_argument("--cache", type=Path, required=True)
parser.add_argument("--data-dir", type=Path, required=True)
parser.add_argument("--count", type=int, default=100)
parser.add_argument("--runs", type=int, default=3)
parser.add_argument("--cpu", action="store_true")
args = parser.parse_args()
if args.runs < 1 or args.count < 1:
    parser.error("count and runs must be positive")
command = [str(args.exe.resolve()), "--cache", str(args.cache.resolve()),
           "--data-dir", str(args.data_dir.resolve()), "--inspect", str(args.count),
           "--query", "red stripes"]
if args.cpu:
    command.append("--cpu")
runs = []
for i in range(args.runs):
    started = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True,
                            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    elapsed = time.perf_counter() - started
    if result.returncode:
        raise SystemExit(f"Benchmark failed ({result.returncode}): {result.stderr}")
    runs.append(elapsed)
    print(json.dumps({"run": i + 1, "seconds": round(elapsed, 3),
                      "details": [line for line in result.stdout.splitlines()
                                  if line.startswith(("Timing:", "Decoded ", "Inference:"))]}), flush=True)
print(json.dumps({"median_seconds": round(statistics.median(runs), 3)}))
