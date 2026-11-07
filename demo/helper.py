#!/usr/bin/env python3
"""Demo helper: serves fictional things from items.json over the real protocol."""

import json
import os
import sys

with open(os.environ["EVERYTHING_DEMO_ITEMS"], encoding="utf-8") as stream:
    raw = json.load(stream)

items = []
for index, item in enumerate(raw):
    row = {
        "uiUuid": "00000000-0000-5000-8000-%012d" % index,
        "context": "", "searchTerms": [], "parentId": "", "badges": [],
        "iconHints": [], "active": False, "recency": 0,
        "activationToken": "demo-%d" % index,
    }
    row.update(item)
    row["id"] = "demo:" + item["id"]
    items.append(row)

print(json.dumps({"version": 1, "type": "ready"}), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    if request.get("type") == "scan":
        print(json.dumps({"version": 1, "type": "snapshot", "requestId": request["id"],
                          "full": True, "items": items, "providers": {}, "warnings": []}), flush=True)
    elif request.get("type") == "shutdown":
        break
