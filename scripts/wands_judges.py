#!/usr/bin/env python3
"""Build a NDJSON judges file from WANDS query and label CSVs.

Each output line has the form:
    {"query": "...", "relevant_ids": ["id1", "id2", ...], "k": 10}

Queries with no Exact or Substitute labels are skipped.

Usage:
    python3 scripts/wands_judges.py \
        --queries data/wands/query.csv \
        --labels  data/wands/label.csv \
        --output  data/wands/judges.ndjson
"""
import argparse
import csv
import json


def parse_args():
    p = argparse.ArgumentParser(description="Build WANDS judges NDJSON file")
    p.add_argument("--queries", required=True, help="Path to query.csv")
    p.add_argument("--labels", required=True, help="Path to label.csv")
    p.add_argument("--output", required=True, help="Output NDJSON file path")
    return p.parse_args()


def main():
    args = parse_args()

    # Load queries: query_id -> query string
    queries = {}
    with open(args.queries, newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            qid = row.get("query_id", "").strip()
            q = row.get("query", "").strip()
            if qid and q:
                queries[qid] = q

    # Load labels: collect relevant product_ids per query_id
    # Relevant = "Exact" or "Substitute"
    relevant: dict[str, list[str]] = {}
    with open(args.labels, newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            qid = row.get("query_id", "").strip()
            pid = row.get("product_id", "").strip()
            label = row.get("label", "").strip()
            if label in ("Exact", "Substitute") and qid and pid:
                relevant.setdefault(qid, []).append(pid)

    # Write NDJSON
    written = 0
    skipped = 0
    with open(args.output, "w", encoding="utf-8") as out:
        for qid, query_text in queries.items():
            relevant_ids = relevant.get(qid)
            if not relevant_ids:
                skipped += 1
                continue
            record = {
                "query": query_text,
                "relevant_ids": relevant_ids,
                "k": 10,
            }
            out.write(json.dumps(record) + "\n")
            written += 1

    print(f"Done. Wrote {written} judged queries, skipped {skipped} with no relevant products.")
    print(f"Output: {args.output}")


if __name__ == "__main__":
    main()
