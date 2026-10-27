import json, subprocess, time
H="/home/adhikasp/Workspace/omarchy-everything/rust/target/release/everything-helper"
p=subprocess.Popen([H,"--json-lines"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
rd=lambda: json.loads(p.stdout.readline())
rd()
p.stdin.write(json.dumps({"version":1,"id":"s","type":"scan","options":{}})+"\n"); p.stdin.flush()
while True:
    m=rd()
    if m["type"]=="snapshot" and m["full"]: break
items=m["items"]
def windows(): return len(json.loads(subprocess.check_output(["hyprctl","-j","clients"])))
def act(item, label):
    n0=windows(); t=time.perf_counter()
    p.stdin.write(json.dumps({"version":1,"id":"a-"+label,"type":"activate","itemId":item["id"],"token":item["activationToken"]})+"\n"); p.stdin.flush()
    while True:
        r=rd()
        if r["type"]=="activation": break
    time.sleep(0.4)
    active=json.loads(subprocess.check_output(["hyprctl","-j","activewindow"]))
    print(f"{label:8s} ok={r['ok']} stale={r['stale']} msg={r['message']!r} {1000*(time.perf_counter()-t):.0f}ms windows {n0}->{windows()} active={active.get('class')}: {active.get('title','')[:50]}")
tab=next(i for i in items if i["kind"]=="browser-tab" and "Bot Watch" in i["title"] or i["kind"]=="browser-tab")
act(tab,"chrome")
agent=next(i for i in items if i["kind"]=="herdr-agent")
act(agent,"herdr")
win=next(i for i in items if i["kind"]=="window" and "ghostty" in i["context"].lower())
act(win,"window")
p.stdin.close()
