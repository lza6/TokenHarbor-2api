import json, time, urllib.request, urllib.error, sys
from concurrent.futures import ThreadPoolExecutor, as_completed

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://52.141.3.10:47830"
KEY  = sys.argv[2] if len(sys.argv) > 2 else "sk-REPLACE_WITH_YOUR_KEY"
N    = int(sys.argv[3]) if len(sys.argv) > 3 else 40

body = json.dumps({"model":"th-rudder:free","messages":[{"role":"user","content":"hi"}],"max_tokens":8,"stream":True}).encode()
req = urllib.request.Request(BASE + "/v1/chat/completions", data=body, method="POST")
req.add_header("Content-Type", "application/json")
req.add_header("Authorization", "Bearer " + KEY)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # 禁用代理直连

def one(i):
    t0 = time.time()
    try:
        with opener.open(req, timeout=240) as r:
            data = r.read().decode("utf-8", "replace")
        return (r.status, "[DONE]" in data, int((time.time()-t0)*1000))
    except urllib.error.HTTPError as e:
        return (e.code, False, int((time.time()-t0)*1000))
    except Exception as e:
        return ("EX:"+type(e).__name__, False, int((time.time()-t0)*1000))

t0 = time.time()
with ThreadPoolExecutor(max_workers=N) as ex:
    futs = [ex.submit(one, i) for i in range(N)]
    res = [f.result() for f in as_completed(futs)]
elapsed = time.time() - t0
ok = [r for r in res if r[0] == 200]
done = [r for r in ok if r[1]]
err = [r for r in res if r[0] != 200]
print(f"total={len(res)} ok={len(ok)} streamDone={len(done)} err={len(err)} wall={elapsed:.1f}s")
lat = sorted(r[2] for r in ok)
if lat:
    p50 = lat[min(len(lat)-1, int(len(lat)*0.5))]
    p95 = lat[min(len(lat)-1, int(len(lat)*0.95))]
    print(f"p50={p50}ms p95={p95}ms max={max(lat)}ms")
if err:
    from collections import Counter
    print("errors:", Counter(str(r[0]) for r in err))
sys.exit(0 if len(err)==0 and len(ok)==N else 1)