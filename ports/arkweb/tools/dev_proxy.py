#!/usr/bin/env python3
"""Minimal HTTP/HTTPS(CONNECT) forward proxy for on-device Servo ArkWeb testing.

The rk3568 dev board has no direct network route, so route Servo's networking through the
development host instead:

    # on the host
    python3 ports/arkweb/tools/dev_proxy.py 8899
    hdc rport tcp:8899 tcp:8899          # device 127.0.0.1:8899 -> host 127.0.0.1:8899

    # on the device (read by the shim at engine init, sets Servo's proxy prefs)
    hdc shell param set web.engine.servo.proxy http://127.0.0.1:8899

Then (re)launch the app; live http/https URLs load through the host's connection. Unset with
`param set web.engine.servo.proxy ""` (takes effect on the next engine init).
"""

import socket
import sys
import threading


def pipe(src: socket.socket, dst: socket.socket) -> None:
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        for sock in (src, dst):
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def handle(client: socket.socket) -> None:
    client.settimeout(30)
    try:
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = client.recv(4096)
            if not chunk:
                return
            head += chunk
        method, target, _ = head.split(b"\r\n", 1)[0].decode("latin1").split(" ", 2)
        if method == "CONNECT":
            host, _, port = target.partition(":")
            upstream = socket.create_connection((host, int(port or 443)), timeout=15)
            client.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        else:
            hostport, _, path = target.split("//", 1)[1].partition("/")
            host, _, port = hostport.partition(":")
            upstream = socket.create_connection((host, int(port or 80)), timeout=15)
            upstream.sendall(head.replace(target.encode(), b"/" + path.encode(), 1))
        threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
        pipe(upstream, client)
    except Exception as exc:  # noqa: BLE001
        print(f"proxy error: {exc}", file=sys.stderr)
    finally:
        client.close()


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8899
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", port))
    server.listen(128)
    print(f"dev proxy listening on 127.0.0.1:{port}", flush=True)
    while True:
        client, _ = server.accept()
        threading.Thread(target=handle, args=(client,), daemon=True).start()


if __name__ == "__main__":
    main()
