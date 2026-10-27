import json, subprocess, sys
def scan(cmd):
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    assert json.loads(p.stdout.readline())["type"] == "ready"
    p.stdin.write(json.dumps({"version":1,"id":"s","type":"scan","options":{"ghostty":False,"ghosttyPalette":False}})+"\n"); p.stdin.flush()
    while True:
        m = json.loads(p.stdout.readline())
        if m["type"]=="snapshot" and m["full"]: break
    p.stdin.close(); p.wait(5); return m
H="/home/adhikasp/Workspace/omarchy-everything"
py = scan(["python3", H+"/helper/everything_helper.py","--json-lines"])
rs = scan([H+"/rust/target/release/everything-helper","--json-lines"])
strip = lambda items: {i["id"]: {k:v for k,v in i.items() if k!="activationToken"} for i in items if i["provider"] in ("Hyprland","Chrome","Herdr")}
a, b = strip(py["items"]), strip(rs["items"])
print("python", len(a), "rust", len(b), "only-py", len(set(a)-set(b)), "only-rust", len(set(b)-set(a)))
diff = 0
for k in sorted(set(a)&set(b)):
    if a[k]!=b[k]:
        diff += 1
        if diff<=4:
            for f in a[k]:
                if a[k][f]!=b[k].get(f): print(" DIFF", k[:50], f, repr(a[k][f])[:80], "|", repr(b[k].get(f))[:80])
print("rows with differences:", diff)
for k in list(set(a)-set(b))[:3]: print(" only py:", k[:70], a[k]["title"][:40])
for k in list(set(b)-set(a))[:3]: print(" only rs:", k[:70], b[k]["title"][:40])
print("warnings py", py["warnings"], "rs", rs["warnings"])
