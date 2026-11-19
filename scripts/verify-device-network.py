"""Exercise separate device profiles, proxy handshakes and workspace-independent settings."""
import argparse
import base64
from contextlib import ExitStack, contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import import_module
from pathlib import Path
from queue import Queue
from socketserver import StreamRequestHandler, ThreadingTCPServer
import tempfile
import threading

release = import_module("verify-release")


@contextmanager
def serve(cls, handler):
    with cls(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server.server_address[1]
        finally:
            server.shutdown()
            thread.join(timeout=3)


def verify(directory):
    cli = release.binary(directory, "shipforge")
    seen_http, seen_local, seen_socks, seen_tls = Queue(), Queue(), Queue(), Queue()

    class Http(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_CONNECT(self):
            seen_http.put((self.path, self.headers.get("Proxy-Authorization"), self.headers.get("Authorization")))
            self.send_response(407)
            self.send_header("Proxy-Authenticate", 'Basic realm="fixture"')
            self.end_headers()

        def do_HEAD(self):
            seen_local.put((self.path, self.headers.get("Proxy-Authorization")))
            self.send_response(204)
            self.end_headers()

    class Socks(StreamRequestHandler):
        def handle(self):
            header = self.rfile.read(2)
            assert header[0] == 5
            assert 0 in self.rfile.read(header[1])
            self.wfile.write(b"\x05\x00")
            assert self.rfile.read(4) == b"\x05\x01\x00\x03"
            host = self.rfile.read(self.rfile.read(1)[0]).decode("ascii")
            seen_socks.put((host, int.from_bytes(self.rfile.read(2), "big")))
            self.wfile.write(b"\x05\x05\x00\x01" + bytes(6))

    class HttpsProxy(StreamRequestHandler):
        def handle(self):
            record = self.rfile.read(5)
            if len(record) != 5:
                return
            payload = self.rfile.read(int.from_bytes(record[3:], "big"))
            seen_tls.put((record[0], payload[:1], b"Proxy-Authorization" in payload))
            # A TLS proxy rejects this connection during the handshake, before CONNECT or credentials.
            self.wfile.write(b"\x15\x03\x03\x00\x02\x02\x30")

    with tempfile.TemporaryDirectory(prefix="shipforge-net-") as tmp, ExitStack() as stack:
        device = Path(tmp) / "device"
        device.mkdir()
        # The diagnostic and updater must still work when a project cannot be opened.
        (device / "workspace.sqlite3").write_bytes(b"not a project")
        aux = Path(tmp) / "other"
        port = stack.enter_context(serve(ThreadingHTTPServer, Http))
        socks = stack.enter_context(serve(ThreadingTCPServer, Socks))
        tls = stack.enter_context(serve(ThreadingTCPServer, HttpsProxy))
        local = f"http://127.0.0.1:{port}"
        target = "https://team.example.test/"

        def call(op, *, ok=True, profile=device, **fields):
            return release.run_cli(cli, profile, "request", request={"op": op, **fields}, ok=ok)

        about = call("device.about")
        assert about["name"] == "ShipForge" and about["version"]
        if (cli.parent / "docs/windows.md").is_file():
            assert about["guide"].startswith("file:"), about
        initial = call("network.settings")
        assert initial["mode"] == "system" and initial["rev"] == 0
        manual = dict(mode="manual", endpoint=local, username="qa", password="q:sp ace")
        error = call("network.test", **manual, test_url=target, ok=False)
        assert "authentication" in error["message"] and "q:sp ace" not in str(error), error
        assert seen_http.get(timeout=4) == ("team.example.test:443", "Basic " + base64.b64encode(b"qa:q:sp ace").decode(), None)
        assert call("network.settings") == initial
        saved = call("network.save", expected_rev=0, **manual)
        assert saved["has_password"] and "q:sp ace" not in str(saved)
        assert call("network.settings", profile=aux)["mode"] == "system"
        assert call("network.test", test_url=local + "/health")["status"] == 204
        assert seen_local.get(timeout=4) == ("/health", None) and seen_http.empty()
        assert call("network.save", mode="direct", expected_rev=0, ok=False)["code"] == "conflict"
        call("network.test", mode="manual", endpoint=f"socks5://127.0.0.1:{socks}", username="", test_url=target, ok=False)
        assert seen_socks.get(timeout=4) == ("team.example.test", 443)
        tls_error = call("network.test", **{**manual, "endpoint": f"https://127.0.0.1:{tls}"}, test_url=target, ok=False)
        assert tls_error["code"] == "offline" and "q:sp ace" not in str(tls_error), tls_error
        assert seen_tls.get(timeout=4) == (22, b"\x01", False)
        assert call("network.settings") == saved
        direct = call("network.save", mode="direct", expected_rev=saved["rev"])
        assert direct["mode"] == "direct" and call("network.test", test_url=local)["reachable"]
        assert call("network.save", mode="manual", endpoint="http://new.example.test:7000", expected_rev=direct["rev"])["has_password"] is False
        assert call("network.test", test_url="https://qa:secret@example.test/", ok=False)["code"] == "invalid"
        assert (device / "workspace.sqlite3").read_bytes() == b"not a project"
    print("Verified isolated device settings, HTTP CONNECT, HTTPS proxy TLS, SOCKS5 remote DNS, loopback bypass, credential scoping, stale changes, and operation with an unreadable project.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    verify(parser.parse_args().directory.resolve())
