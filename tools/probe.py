#!/usr/bin/env python3
"""Synthetic Bedrock logins for testing horizon-floodgate on a throwaway server (needs `cryptography`).

  probe.py geyser <proxy port> <key file> <case>
      Acts as Geyser: joins a Velocity proxy that runs Floodgate, with Bedrock data in the handshake host.
  probe.py proxy <server port> <key file> <forwarding secret file> <case>
      Acts as a Floodgate proxy: joins a Horizon server directly, with Floodgate data in the host and the
      player's profile in Velocity modern forwarding (version 1), signed with the given secret.

Cases: plain (a Bedrock player), linked (linked to a Java account: the data is longer than 255 characters).
Prints what the server answered: "joined" once the play state is reached, or the disconnect reason.
"""
import base64, hashlib, hmac, os, socket, struct, sys, time, uuid, zlib

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

PROTOCOL = 777  # 26.3


def varint(n):
    n &= 0xFFFFFFFF
    out = b""
    while True:
        b, n = n & 0x7F, n >> 7
        out += bytes([b | 0x80]) if n else bytes([b])
        if not n:
            return out


def parse_varint(buf, i=0):
    n = shift = 0
    while True:
        b = buf[i]; i += 1
        n |= (b & 0x7F) << shift
        if not b & 0x80:
            return n, i
        shift += 7


def string(s):
    b = s.encode() if isinstance(s, str) else s
    return varint(len(b)) + b


def encrypt(key, text):
    """GeyserMC's AesCipher with Base64Topping."""
    iv = os.urandom(12)
    sealed = AESGCM(key).encrypt(iv, text.encode(), None)
    return b"^Floodgate^>" + base64.b64encode(iv) + b"!" + base64.b64encode(sealed)


def bedrock_data(case, from_proxy):
    linked = "null"
    name, xuid = "Tester", "2535400000000001"
    if case == "linked":
        linked = "JavaFriend;11111111-2222-3333-4444-555555555555;00000000-0000-0000-0009-01eed05d1001"
    return "\0".join(["1.21.130", name, xuid, "7", "en_US", "0", "1", "203.0.113.9", linked,
                      "1" if from_proxy else "0", "-1", "verify-code"])  # subscribe id -1: no skin socket


def java_identity(case):
    if case == "linked":
        return "JavaFriend", uuid.UUID("11111111-2222-3333-4444-555555555555")
    return ".Tester", uuid.UUID(int=2535400000000001)


class Conn:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port), timeout=15)
        self.t = None
        self.buf = b""

    def exact(self, n):
        while len(self.buf) < n:
            c = self.s.recv(65536)
            if not c:
                raise EOFError("connection closed")
            self.buf += c
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def vi(self):
        n = shift = 0
        while True:
            b = self.exact(1)[0]
            n |= (b & 0x7F) << shift
            if not b & 0x80:
                return n
            shift += 7

    def send(self, pid, body=b""):
        data = varint(pid) + body
        if self.t is not None and self.t >= 0:
            data = varint(len(data)) + zlib.compress(data) if len(data) >= self.t else varint(0) + data
        self.s.sendall(varint(len(data)) + data)

    def recv(self):
        data = self.exact(self.vi())
        if self.t is not None and self.t >= 0:
            size, i = parse_varint(data)
            data = zlib.decompress(data[i:]) if size else data[i:]
        pid, i = parse_varint(data)
        return pid, data[i:]


def forwarding(secret, name, profile_uuid):
    data = varint(1) + string("203.0.113.9") + profile_uuid.bytes + string(name) + varint(0)
    return hmac.new(secret, data, hashlib.sha256).digest() + data


def login(port, host, name, profile_uuid, secret=None):
    c = Conn(port)
    c.send(0, varint(PROTOCOL) + string(host) + struct.pack(">H", port) + varint(2))
    c.send(0, string(name) + profile_uuid.bytes)
    try:
        while True:  # login
            pid, d = c.recv()
            if pid == 0:
                return f"disconnected at login: {d.decode(errors='replace')}"
            if pid == 3:
                c.t = parse_varint(d)[0]
            elif pid == 4:  # login plugin request: the Velocity forwarding question
                mid, i = parse_varint(d)
                channel_len, i = parse_varint(d, i)
                channel = d[i:i + channel_len].decode()
                if channel == "velocity:player_info" and secret is not None:
                    c.send(2, varint(mid) + b"\x01" + forwarding(secret, name, profile_uuid))
                else:
                    c.send(2, varint(mid) + b"\x00")
            elif pid == 2:
                got = uuid.UUID(bytes=d[:16])
                gname_len, i = parse_varint(d, 16)
                gname = d[i:i + gname_len].decode()
                break
        c.send(3)  # login acknowledged
        while True:  # configuration
            pid, d = c.recv()
            if pid == 15:  # select known packs
                c.send(7, varint(0))
            elif pid == 4:  # keep alive
                c.send(4, d)
            elif pid == 5:  # ping
                c.send(5, d)
            elif pid == 3:  # finish configuration
                c.send(3)
                break
            elif pid == 2:
                return f"disconnected in configuration: {d!r}"
        pid, d = c.recv()
        return f"joined as {gname} {got} (first play packet 0x{pid:02x})"
    except (EOFError, ConnectionError, socket.timeout) as e:
        return f"closed without a reason: {e}"
    finally:
        time.sleep(1.0)
        c.s.close()


if __name__ == "__main__":
    mode = sys.argv[1]
    key = open(sys.argv[3], "rb").read()
    case = sys.argv[-1]
    if mode == "geyser":
        host = b"127.0.0.1\0" + encrypt(key, bedrock_data(case, False))
        print(case, "->", login(int(sys.argv[2]), host, "Tester", uuid.UUID(int=0)))
    else:
        secret = open(sys.argv[4], "rb").read().strip()
        host = b"127.0.0.1\0" + encrypt(key, bedrock_data(case, True))
        name, profile_uuid = java_identity(case)
        print(case, f"(host {len(host)} bytes) ->", login(int(sys.argv[2]), host, name, profile_uuid, secret))
