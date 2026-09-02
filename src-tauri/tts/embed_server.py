# Tiny local embedding server for the project library's semantic search
# (library.rs's `search_projects`) -- loads `all-MiniLM-L6-v2` once and
# serves embedding requests over plain HTTP, the same "start once, reuse
# for the app's whole lifetime" shape as llm.rs's llama-server, and for the
# identical reason: the model takes ~19s to load (confirmed directly), so a
# one-shot-subprocess-per-call script (this app's usual pattern, e.g.
# generate_music.py) would make every search query pay that cost, which is
# not tolerable for something the user is actively waiting on while typing.
#
# Deliberately stdlib-only (http.server + socketserver, no Flask/FastAPI) --
# `sentence-transformers` is the only new dependency this feature needs, and
# a raw HTTP server this simple (two routes, JSON in/out) doesn't earn a
# whole extra web-framework dependency.
#
# Protocol: GET /health -> 200 once the model is loaded; POST /embed with
# JSON body {"text": "..."} -> {"embedding": [384 floats]}. Errors come back
# as a non-2xx status with a JSON {"error": "..."} body.

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL_NAME = "all-MiniLM-L6-v2"

print(f"Loading {MODEL_NAME}...", flush=True)
from sentence_transformers import SentenceTransformer

model = SentenceTransformer(MODEL_NAME)
print("Model loaded.", flush=True)


class Handler(BaseHTTPRequestHandler):
    # Quiets the default per-request access log line -- this server gets
    # hit on every keystroke of a search box, and stderr noise there isn't
    # useful the way it is for the occasional music/voiceover generation call.
    def log_message(self, format, *args):
        pass

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._send_json(200, {"status": "ok"})
        else:
            self._send_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/embed":
            self._send_json(404, {"error": "not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length) or b"{}")
            text = body.get("text", "")
            if not isinstance(text, str) or not text.strip():
                self._send_json(400, {"error": "'text' must be a non-empty string"})
                return
            embedding = model.encode(text).tolist()
            self._send_json(200, {"embedding": embedding})
        except Exception as e:
            self._send_json(500, {"error": str(e)})


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8735
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"Listening on 127.0.0.1:{port}", flush=True)
    server.serve_forever()
