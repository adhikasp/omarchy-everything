import json, subprocess, sys, time, statistics as st
H="/home/adhikasp/Workspace/omarchy-everything"
CMD=[H+"/rust/target/release/everything-helper","--json-lines"]
def once(cmd):
    t0=time.perf_counter()
    p=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
    rd=lambda: json.loads(p.stdout.readline())
    assert rd()["type"]=="ready"; m={"ready":time.perf_counter()-t0}
    def scan(i):
        p.stdin.write(json.dumps({"version":1,"id":f"s{i}","type":"scan","options":{"ghostty":False,"ghosttyPalette":False}})+"\n"); p.stdin.flush()
        first=None; t=time.perf_counter()
        while True:
            x=rd()
            if x["type"]!="snapshot": continue
            if first is None: first=time.perf_counter()-t
            if x["full"]: return first, time.perf_counter()-t
    f,full=scan(1); m["first_rows_after_scan"]=f; m["cold_full_scan"]=full; m["cold_total_to_full"]=time.perf_counter()-t0
    f,full=scan(2); m["warm_first_rows"]=f; m["warm_full_scan"]=full
    rss=int(open(f"/proc/{p.pid}/status").read().split("VmRSS:")[1].split()[0]); m["rss_mb"]=rss/1024/1000*1000/1000*1000/1000 if False else rss/1024
    p.stdin.close(); p.wait(5); return m
N=int(sys.argv[1]) if len(sys.argv)>1 else 15
res=[once(CMD) for _ in range(N)]
keys=["ready","cold_total_to_full","first_rows_after_scan","warm_first_rows","warm_full_scan","rss_mb"]
for k in keys:
    u=1 if k=="rss_mb" else 1000
    print(f"{k:24s}{st.median(r[k] for r in res)*u:10.1f}")
