#!/usr/bin/env python3
"""Run a long job with a memory guard, so it can't take the machine down.

    train/guard.py [--max-gb 4] [--min-free-pct 12] -- <command ...>

This 16 GB Air runs with swap off and often in Low Power Mode. With no swap,
a runaway process starves the kernel: on 2026-11-10 `dump_training` reached
12 GB resident (decoding multi-hour renders whole), watchdogd missed its
check-ins for 90 s, and the kernel panicked.

The guard:
* warns if swap is disabled or Low Power Mode is on;
* holds `caffeinate` for the job's lifetime;
* runs the command in its own process group, and every 0.5 s sums the RSS
  of the group, KILLING it if the sum passes --max-gb or system free memory
  (memory_pressure) falls under --min-free-pct. It exits 137 and says which
  limit fired.
"""

import argparse
import os
import re
import signal
import subprocess
import sys
import time


def sh(cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout


def group_rss_gb(pgid):
    total = 0
    for line in sh(["ps", "-A", "-o", "pgid=,rss="]).splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == str(pgid):
            total += int(parts[1])
    return total / 1e6  # KiB → GB


def free_pct():
    m = re.search(r"free percentage:\s*(\d+)%", sh(["memory_pressure"]))
    return int(m.group(1)) if m else 100


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-gb", type=float, default=4.0)
    ap.add_argument("--min-free-pct", type=int, default=12)
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        ap.error("no command")

    if "total = 0.00M" in sh(["sysctl", "vm.swapusage"]):
        print("guard: WARNING swap is disabled — the guard is the only backstop", file=sys.stderr)
    if re.search(r"lowpowermode\s+1", sh(["pmset", "-g"])):
        print("guard: WARNING Low Power Mode is on", file=sys.stderr)

    job = subprocess.Popen(cmd, start_new_session=True)
    awake = subprocess.Popen(["caffeinate", "-dimsu", "-w", str(job.pid)])
    peak, fired = 0.0, None
    try:
        while job.poll() is None:
            rss = group_rss_gb(job.pid)
            peak = max(peak, rss)
            if rss > a.max_gb:
                fired = f"job RSS {rss:.2f} GB > {a.max_gb} GB"
            else:
                free = free_pct()
                if free < a.min_free_pct:
                    fired = f"system free memory {free}% < {a.min_free_pct}%"
            if fired:
                os.killpg(job.pid, signal.SIGKILL)
                job.wait()
                break
            time.sleep(0.5)
    except KeyboardInterrupt:
        os.killpg(job.pid, signal.SIGKILL)
        job.wait()
        raise
    finally:
        awake.terminate()
    if fired:
        print(f"guard: KILLED — {fired} (peak {peak:.2f} GB)", file=sys.stderr)
        sys.exit(137)
    print(f"guard: done, exit {job.returncode}, peak RSS {peak:.2f} GB", file=sys.stderr)
    sys.exit(job.returncode)


if __name__ == "__main__":
    main()
