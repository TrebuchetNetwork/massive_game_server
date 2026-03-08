#!/usr/bin/env python3
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


OUTPUT_PATH = Path(sys.argv[1] if len(sys.argv) > 1 else "/data/received.jsonl")
PORT = int(sys.argv[2] if len(sys.argv) > 2 else "18081")


class AlertWebhookHandler(BaseHTTPRequestHandler):
    def _write(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/healthz":
            self._write(200, {"ok": True})
            return
        self._write(404, {"ok": False, "error": "not_found"})

    def do_POST(self):
        if self.path != "/alerts":
            self._write(404, {"ok": False, "error": "not_found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        try:
            payload = json.loads(raw.decode("utf-8"))
        except Exception:
            self._write(400, {"ok": False, "error": "invalid_json"})
            return

        OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
        with OUTPUT_PATH.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload))
            handle.write("\n")
        self._write(200, {"ok": True})

    def log_message(self, fmt, *args):
        return


def main():
    server = ThreadingHTTPServer(("0.0.0.0", PORT), AlertWebhookHandler)
    server.serve_forever()


if __name__ == "__main__":
    main()
