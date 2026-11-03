"""Local transport controls for provider proof discovery."""
import contextlib
import io
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading
import unittest
from unittest.mock import patch
import urllib.request

import test_source_proof as support

PROOF = support.PROOF


@contextlib.contextmanager
def local_server(handler):
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        thread.join(timeout=2)
        server.server_close()
        if thread.is_alive():
            raise RuntimeError("local transport fixture did not stop")


class TransportTests(unittest.TestCase):
    def test_redirect_returns_archive_without_forwarding_authorization(self):
        received = []
        archive = b"owned archive bytes\x00\xff"

        class Origin(BaseHTTPRequestHandler):
            redirect_url = ""
            def do_GET(self):
                received.append(("origin", self.headers.get("Authorization")))
                self.send_response(302)
                self.send_header("Location", self.redirect_url)
                self.end_headers()
            def log_message(self, *_args):
                pass

        class Destination(BaseHTTPRequestHandler):
            def do_GET(self):
                received.append(("destination", self.headers.get("Authorization")))
                self.send_response(200)
                self.send_header("Content-Length", str(len(archive)))
                self.end_headers()
                self.wfile.write(archive)
            def log_message(self, *_args):
                pass

        with local_server(Destination) as destination, local_server(Origin) as origin:
            Origin.redirect_url = f"http://127.0.0.1:{destination.server_port}/archive"
            opener = urllib.request.build_opener(PROOF.NoCredentialRedirect())
            for unredirected in (False, True):
                with self.subTest(unredirected=unredirected):
                    received.clear()
                    request = urllib.request.Request(f"http://127.0.0.1:{origin.server_port}/artifact")
                    if unredirected:
                        request.add_unredirected_header("Authorization", "Bearer fixture-only")
                    else:
                        request.add_header("Authorization", "Bearer fixture-only")
                    with opener.open(request, timeout=3) as response:
                        self.assertEqual(response.read(), archive)
                    self.assertEqual(received, [("origin", "Bearer fixture-only"), ("destination", None)])

    def test_fallback_reports_only_exception_class(self):
        output, error = io.StringIO(), io.StringIO()
        with patch.object(PROOF, "context", return_value=support.current()), \
             patch.object(PROOF, "Client", side_effect=AttributeError("fixture-secret https://invalid.example/private")), \
             patch.object(PROOF.sys, "argv", ["source-proof.py", "find", "ci"]), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            PROOF.main()
        self.assertEqual(output.getvalue(), "reuse=false\n")
        self.assertEqual(error.getvalue(), "No compatible provider proof (AttributeError); checks will execute.\n")


if __name__ == "__main__":
    unittest.main()
