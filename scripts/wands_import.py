#!/usr/bin/env python3
"""Import WANDS products into a running Vectoria server.

Usage:
    python3 scripts/wands_import.py \
        --products data/wands/product.csv \
        --server http://localhost:7700 \
        --api-key your-key \
        --max-products 42994
"""
import argparse
import csv
import json
import sys
import urllib.request
import urllib.error


def parse_args():
    p = argparse.ArgumentParser(description="Import WANDS products into Vectoria")
    p.add_argument("--products", required=True, help="Path to product.csv")
    p.add_argument("--server", required=True, help="Vectoria server base URL")
    p.add_argument("--api-key", required=True, help="Vectoria API key")
    p.add_argument("--max-products", type=int, default=42994,
                   help="Maximum number of products to import")
    return p.parse_args()


def post_product(server, api_key, payload):
    url = server.rstrip("/") + "/products"
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.status
    except urllib.error.HTTPError as e:
        return e.code


def main():
    args = parse_args()

    imported = 0
    errors = 0

    with open(args.products, newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            if imported >= args.max_products:
                break

            product_id = row.get("product_id", "").strip()
            name = row.get("product_name", "").strip()
            cls = row.get("product_class", "").strip()
            description = row.get("description", "").strip()

            if not product_id:
                continue

            # Build text field: name + class + description
            parts = [p for p in [name, cls, description] if p]
            text = " ".join(parts)

            payload = {
                "id": product_id,
                "text": text,
                "metadata": {
                    "product_name": name,
                    "product_class": cls,
                    "category_hierarchy": row.get("category_hierarchy", "").strip(),
                    "description": description,
                },
            }

            status = post_product(args.server, args.api_key, payload)
            if status in (200, 201):
                imported += 1
            else:
                errors += 1
                print(f"  WARN: product {product_id} returned HTTP {status}",
                      file=sys.stderr)

            if imported % 500 == 0 and imported > 0:
                print(f"  Imported {imported} products...", flush=True)

    print(f"Done. Imported {imported} products, {errors} errors.")


if __name__ == "__main__":
    main()
