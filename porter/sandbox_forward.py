"""Runs inside a sandbox: forwards 127.0.0.1:3128 to the host proxy's Unix socket.

Uses only the standard library, because it runs before anything else is set up.
"""

import os
import select
import socket
import threading

LISTEN = ("127.0.0.1", 3128)
SOCKET_PATH = os.environ.get("PORTER_PROXY_SOCKET", "/proxy/proxy.sock")


def bridge(client: socket.socket) -> None:
    upstream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        upstream.connect(SOCKET_PATH)
        pair = [client, upstream]
        while True:
            readable, _, _ = select.select(pair, [], [], 600)
            if not readable:
                return
            for source in readable:
                data = source.recv(65536)
                if not data:
                    return
                (upstream if source is client else client).sendall(data)
    except OSError:
        return
    finally:
        client.close()
        upstream.close()


def main() -> None:
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(LISTEN)
    server.listen(64)
    while True:
        client, _ = server.accept()
        threading.Thread(target=bridge, args=(client,), daemon=True).start()


if __name__ == "__main__":
    main()
