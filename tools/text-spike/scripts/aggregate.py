#!/usr/bin/env python3
"""Aggregates a measurements.jsonl (from run_measurements.sh) into the
median/min/max tables docs/research/text-stack-2026.md quotes.

Usage:
    python3 tools/text-spike/scripts/aggregate.py tools/text-spike/results/measurements.jsonl
"""

import json
import statistics
import sys
from collections import Counter, defaultdict


def load(path):
    groups = defaultdict(list)
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            key = (d["backend"], d["corpus"], d["size"], d["cache"])
            groups[key].append(d)
    return groups


def main():
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(1)
    groups = load(sys.argv[1])

    print(f"Loaded {sum(len(v) for v in groups.values())} points, {len(groups)} groups")
    print()

    header = f"{'backend':<12}{'corpus':<14}{'size':<11}{'cache':<7}{'n':<3}{'med_ms':>10}{'min_ms':>10}{'max_ms':>10}{'init_MB':>9}{'peak_MB':>9}{'delta_MB':>10}  lines  glyphs"
    print(header)
    for key in sorted(groups):
        backend, corpus, size, cache = key
        items = groups[key]
        elapsed = sorted(i["elapsed_ms"] for i in items)
        init_rss = sorted(i.get("init_rss_bytes", 0) for i in items)
        peak_rss = sorted(i["peak_rss_bytes"] for i in items)
        med_e = statistics.median(elapsed)
        med_init = statistics.median(init_rss)
        med_peak = statistics.median(peak_rss)
        print(
            f"{backend:<12}{corpus:<14}{size:<11}{cache:<7}{len(items):<3}"
            f"{med_e:>10.3f}{elapsed[0]:>10.3f}{elapsed[-1]:>10.3f}"
            f"{med_init / 1e6:>9.1f}{med_peak / 1e6:>9.1f}{(med_peak - med_init) / 1e6:>10.1f}"
            f"  {items[0]['line_count']}  {items[0]['glyph_count']}"
        )

    print()
    print("=== work parity: line_count / glyph_count, size=Large cache=Cold ===")
    by_corpus = defaultdict(dict)
    for key, items in groups.items():
        backend, corpus, size, cache = key
        if size != "Large" or cache != "Cold":
            continue
        by_corpus[corpus][backend] = (items[0]["line_count"], items[0]["glyph_count"])
    for corpus, d in sorted(by_corpus.items()):
        c_lines, c_glyphs = d.get("CosmicText", (None, None))
        p_lines, p_glyphs = d.get("Parley", (None, None))
        diff = 100 * (p_glyphs - c_glyphs) / c_glyphs if c_glyphs else None
        print(
            f"{corpus:<14} cosmic lines={c_lines} glyphs={c_glyphs}  |  "
            f"parley lines={p_lines} glyphs={p_glyphs}  |  glyph diff={diff:+.2f}%"
        )


if __name__ == "__main__":
    main()
