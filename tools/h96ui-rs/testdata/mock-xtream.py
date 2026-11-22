#!/usr/bin/env python3
"""Mock Xtream Codes panel for testing h96ui (NOTES §38). Account demo / demo123.
Refuses clients whose User-Agent does not start with "VLC" (403), like picky real panels, so a
passing test proves h96ui's User-Agent reaches every request (API and streams). Streams are
302 redirects: live to public test HLS, films/episodes to official Blender progressive MP4s. Usage: mock-xtream.py [port]  (default 8766)"""
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

USER, PASS = "demo", "demo123"
BBB = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8"
MP4_BBB = "http://distribution.bbb3d.renderfarming.net/video/mp4/bbb_sunflower_1080p_30fps_normal.mp4"
MP4_SINTEL = "https://download.blender.org/durian/trailer/sintel_trailer-1080p.mp4"
BIPBOP = "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8"
LIVE_CATS = [{"category_id": "1", "category_name": "News", "parent_id": 0},
             {"category_id": "2", "category_name": "Sports", "parent_id": 0}]
LIVE = [{"num": 1, "name": "Mock News One", "stream_type": "live", "stream_id": 101, "category_id": "1"},
        {"num": 2, "name": "Mock News Two", "stream_type": "live", "stream_id": "102", "category_id": "1"},
        {"num": 3, "name": "Mock Sports", "stream_type": "live", "stream_id": 201, "category_id": "2"}]
VOD_CATS = [{"category_id": "10", "category_name": "Animation"}]
VOD = [{"num": 1, "name": "Big Buck Bunny (mock film)", "stream_type": "movie", "stream_id": 1001,
        "category_id": "10", "container_extension": "mp4", "rating": "7.5"}]
SER_CATS = [{"category_id": "20", "category_name": "Test series"}]
SERIES = [{"num": 1, "name": "Mock Show", "series_id": 3001, "category_id": "20"}]
EPISODES = {"1": [{"id": "5002", "episode_num": 2, "title": "Second", "container_extension": "mp4",
                   "info": {"duration_secs": 1800}, "season": 1},
                  {"id": "5001", "episode_num": 1, "title": "Pilot", "container_extension": "mp4",
                   "info": {"duration_secs": 635}, "season": 1}],
            "2": [{"id": "5101", "episode_num": 1, "title": "Season two opener",
                   "container_extension": "mkv", "info": {"duration_secs": 635}, "season": 2}]}
# live = HLS; films and episodes = progressive MP4 with range requests, like real panels
TARGET = {"101": BBB, "102": BIPBOP, "201": BBB, "1001": MP4_BBB, "5001": MP4_SINTEL,
          "5002": MP4_SINTEL, "5101": MP4_SINTEL}

class H(BaseHTTPRequestHandler):
    def send(self, code, body=b"", ctype="application/json", loc=None):
        self.send_response(code)
        if loc: self.send_header("Location", loc)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urlparse(self.path); q = {k: v[0] for k, v in parse_qs(u.query).items()}
        ua = self.headers.get("User-Agent", "")
        print(f"{self.command} {u.path} action={q.get('action', '-')} UA={ua!r}", flush=True)
        if not ua.startswith("VLC"):
            return self.send(403, b"forbidden player", "text/plain")
        if u.path == "/player_api.php":
            if q.get("username") != USER or q.get("password") != PASS:
                return self.send(200, json.dumps({"user_info": {"auth": 0}}).encode())
            a, cat = q.get("action"), q.get("category_id")
            pick = lambda l: [x for x in l if cat is None or x["category_id"] == cat]
            data = {
                None: {"user_info": {"username": USER, "password": PASS, "auth": 1, "status": "Active",
                                     "exp_date": "1798761600", "is_trial": "0", "active_cons": "0",
                                     "max_connections": "2", "allowed_output_formats": ["m3u8", "ts"]},
                       "server_info": {"url": "mock", "port": "8766", "timezone": "Africa/Casablanca"}},
                "get_live_categories": LIVE_CATS, "get_live_streams": pick(LIVE),
                "get_vod_categories": VOD_CATS, "get_vod_streams": pick(VOD),
                "get_series_categories": SER_CATS, "get_series": pick(SERIES),
                "get_series_info": {"info": {"name": "Mock Show"}, "episodes": EPISODES},
            }.get(a, [])
            return self.send(200, json.dumps(data).encode())
        p = u.path.split("/")   # /live|movie|series/USER/PASS/ID.ext
        if len(p) == 5 and p[1] in ("live", "movie", "series") and p[2] == USER and p[3] == PASS:
            t = TARGET.get(p[4].split(".")[0])
            if t: return self.send(302, loc=t)
        self.send(404, b"not found", "text/plain")

    def log_message(self, *a): pass

if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8766
    ThreadingHTTPServer(("0.0.0.0", port), H).serve_forever()
