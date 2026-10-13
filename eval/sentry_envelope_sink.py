"""A local sink for Sentry envelopes — the evidence behind the README's "metadata only" claim.

Run this, point the daemon at it (`SENTRY_DSN=http://<key>@127.0.0.1:8899/<project>`), run one
recap, and read scratch/sentry-sink.log: the transaction, its spans, and their data fields are
there in full, so anyone can check that no clinical text is in them — without an account, and
without taking the vendor's word for it.
"""
import json
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = str(Path(__file__).resolve().parent.parent / "scratch" / "sentry-sink.log")

class Sink(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        body = self.rfile.read(n).decode("utf-8", "replace")
        with open(LOG, "a") as f:
            f.write(f"\n===== POST {self.path} ({n} bytes) =====\n")
            f.write("headers: " + json.dumps({k: v for k, v in self.headers.items()}) + "\n")
            f.write(body + "\n")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"id":"00000000000000000000000000000000"}')

    def log_message(self, *a):
        pass

ThreadingHTTPServer(("127.0.0.1", 8899), Sink).serve_forever()
