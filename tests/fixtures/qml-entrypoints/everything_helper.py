#!/usr/bin/env python3
"""QML transport fixture: record requests without touching the live desktop."""

import json
import os
import sys


print(json.dumps({"version": 1, "type": "ready"}), flush=True)
with open(os.environ["EVERYTHING_REQUEST_LOG"], "a", encoding="utf-8") as output:
    for line in sys.stdin:
        request = json.loads(line)
        output.write(line)
        output.flush()
        if request["type"] == "shutdown":
            break
