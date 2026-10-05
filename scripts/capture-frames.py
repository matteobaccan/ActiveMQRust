# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Development tool: a TCP proxy that records the bytes a real ActiveMQ Java client sends.
# Each client connection is saved as tests/data/golden/<n>.bin (the raw client-to-broker stream).
#
#   python scripts/capture-frames.py --listen 61617 --target 127.0.0.1:61616 --out tests/data/golden
#
# Then run the Java program against tcp://127.0.0.1:61617 and stop the proxy with Ctrl+C.

import argparse
import os
import socket
import threading


def pump(src, dst, record=None):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            if record is not None:
                record.write(data)
            dst.sendall(data)
    except OSError:
        pass
    finally:
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", type=int, default=61617)
    ap.add_argument("--target", default="127.0.0.1:61616")
    ap.add_argument("--out", default=os.path.join("tests", "data", "golden"))
    a = ap.parse_args()
    host, port = a.target.split(":")
    os.makedirs(a.out, exist_ok=True)
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", a.listen))
    srv.listen(64)
    n = len([f for f in os.listdir(a.out) if f.endswith(".bin")])
    print(f"recording on 127.0.0.1:{a.listen} -> {a.target}, files in {a.out}")
    while True:
        client, _ = srv.accept()
        upstream = socket.create_connection((host, int(port)))
        n += 1
        f = open(os.path.join(a.out, f"{n:03d}.bin"), "wb")
        threading.Thread(target=lambda: (pump(client, upstream, f), f.close()), daemon=True).start()
        threading.Thread(target=pump, args=(upstream, client), daemon=True).start()


if __name__ == "__main__":
    main()
