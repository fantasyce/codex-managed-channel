#!/usr/bin/env python3
"""Run a redacted app-server and optional Computer Use acceptance probe."""

import argparse
import base64
import hashlib
import json
import os
import queue
import shlex
import subprocess
import threading
import time


MAGIC = bytes.fromhex("b743b2f7a16572cb")
LOGIN = r'''if [ -z "$SHELL" ] || [ ! -x "$SHELL" ]; then exit 127; fi; CODEX_REMOTE_PAYLOAD="$1"; export CODEX_REMOTE_PAYLOAD; exec "$SHELL" -l -i -c 'CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"; export CODEX_HOME; exec /bin/sh -c "$CODEX_REMOTE_PAYLOAD"' '''
PAYLOAD = r'''printf '%b' '\267\103\262\367\241\145\162\313'; PATH="${CODEX_INSTALL_DIR:-$HOME/.local/bin}:$PATH"; export PATH; exec codex app-server proxy'''


class Connection:
    def __init__(self, host, ssh_config=None):
        remote = "sh -c " + shlex.quote(LOGIN) + " sh " + shlex.quote(PAYLOAD)
        command = ["ssh"]
        if ssh_config:
            command.extend(["-F", ssh_config])
        command.extend(["-T", "-o", "BatchMode=yes", host, remote])
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if self.process.stdout.read(8) != MAGIC:
            raise RuntimeError("Desktop bootstrap probe failed")
        self._upgrade()
        self.messages = queue.Queue()
        self.seen = []
        threading.Thread(target=self._reader, daemon=True).start()

    def _upgrade(self):
        key = base64.b64encode(os.urandom(16)).decode()
        request = (
            "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ).encode()
        self.process.stdin.write(request)
        self.process.stdin.flush()
        response = bytearray()
        while b"\r\n\r\n" not in response and len(response) < 16384:
            value = self.process.stdout.read(1)
            if not value:
                raise RuntimeError("connection closed during WebSocket upgrade")
            response.extend(value)
        expected = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        )
        if not response.startswith(b"HTTP/1.1 101") or expected.lower() not in response.lower():
            raise RuntimeError("WebSocket upgrade was rejected")

    @staticmethod
    def _frame(opcode, payload=b""):
        mask = os.urandom(4)
        length = len(payload)
        header = bytearray([0x80 | opcode])
        if length <= 125:
            header.append(0x80 | length)
        elif length <= 65535:
            header.extend((0x80 | 126, *length.to_bytes(2, "big")))
        else:
            header.append(0x80 | 127)
            header.extend(length.to_bytes(8, "big"))
        header.extend(mask)
        header.extend(value ^ mask[index % 4] for index, value in enumerate(payload))
        return bytes(header)

    def send(self, value):
        payload = json.dumps(value, separators=(",", ":")).encode()
        self.process.stdin.write(self._frame(0x1, payload))
        self.process.stdin.flush()

    def _read_exact(self, size):
        data = bytearray()
        while len(data) < size:
            part = self.process.stdout.read(size - len(data))
            if not part:
                raise EOFError("WebSocket closed")
            data.extend(part)
        return bytes(data)

    def _reader(self):
        fragments = bytearray()
        text_frame = False
        try:
            while True:
                first, second = self._read_exact(2)
                final = bool(first & 0x80)
                opcode = first & 0x0F
                length = second & 0x7F
                if length == 126:
                    length = int.from_bytes(self._read_exact(2), "big")
                elif length == 127:
                    length = int.from_bytes(self._read_exact(8), "big")
                mask = self._read_exact(4) if second & 0x80 else None
                payload = bytearray(self._read_exact(length))
                if mask:
                    for index in range(len(payload)):
                        payload[index] ^= mask[index % 4]
                if opcode == 0x9:
                    self.process.stdin.write(self._frame(0xA, bytes(payload)))
                    self.process.stdin.flush()
                    continue
                if opcode == 0x8:
                    return
                if opcode in (0x1, 0x2):
                    fragments = payload
                    text_frame = opcode == 0x1
                elif opcode == 0x0:
                    fragments.extend(payload)
                else:
                    continue
                if final and text_frame:
                    self.messages.put(json.loads(fragments.decode()))
                    fragments = bytearray()
                    text_frame = False
        except Exception:
            self.messages.put({"probeReaderFailed": True})

    def response(self, request_id, timeout=180):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            message = self.messages.get(timeout=max(0.1, deadline - time.monotonic()))
            if message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError("app-server request failed")
                return message
            self.seen.append(message)
        raise TimeoutError("app-server response timed out")

    def close(self):
        if self.process.poll() is None:
            try:
                self.process.stdin.write(self._frame(0x8))
                self.process.stdin.flush()
                self.process.stdin.close()
            except Exception:
                pass
        try:
            return self.process.wait(timeout=70)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            return self.process.wait(timeout=10)


def contains_tool_evidence(value):
    if isinstance(value, dict):
        encoded = json.dumps(value, separators=(",", ":")).lower()
        if "toolcall" in str(value.get("type", "")).lower() and (
            "cua_repl" in encoded or "cua.getstate" in encoded
        ):
            return True
        return any(contains_tool_evidence(child) for child in value.values())
    if isinstance(value, list):
        return any(contains_tool_evidence(child) for child in value)
    return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", required=True)
    parser.add_argument("--ssh-config")
    parser.add_argument("--cwd")
    parser.add_argument("--computer-use", action="store_true")
    args = parser.parse_args()
    connection = Connection(args.host, args.ssh_config)
    initialized = False
    turn_completed = False
    tool_evidence = False
    try:
        connection.send(
            {
                "method": "initialize",
                "id": 1,
                "params": {
                    "capabilities": {"experimentalApi": True},
                    "clientInfo": {"name": "managed-channel-probe", "version": "0.2.0"},
                },
            }
        )
        connection.response(1)
        connection.send({"method": "initialized", "params": {}})
        initialized = True
        if args.computer_use:
            if not args.cwd:
                raise ValueError("--cwd is required with --computer-use")
            connection.send(
                {"method": "thread/start", "id": 2, "params": {"cwd": args.cwd, "ephemeral": True}}
            )
            thread = connection.response(2).get("result", {}).get("thread", {})
            thread_id = thread.get("id")
            if not thread_id:
                raise RuntimeError("ephemeral task was not created")
            connection.send(
                {
                    "method": "turn/start",
                    "id": 3,
                    "params": {
                        "threadId": thread_id,
                        "input": [{"type": "text", "text": "Use cua_repl exactly once to call cua.getState(). Read only, then confirm briefly."}],
                    },
                }
            )
            connection.response(3)
            deadline = time.monotonic() + 300
            while time.monotonic() < deadline:
                message = connection.messages.get(timeout=max(0.1, deadline - time.monotonic()))
                tool_evidence = tool_evidence or contains_tool_evidence(message)
                if message.get("method") == "turn/completed":
                    turn_completed = True
                    break
        exit_code = connection.close()
    finally:
        if connection.process.poll() is None:
            connection.process.terminate()
    result = {
        "initialized": initialized,
        "computerUseRequested": args.computer_use,
        "turnCompleted": turn_completed,
        "toolEvidence": tool_evidence,
        "cleanExit": exit_code == 0,
    }
    print(json.dumps(result, separators=(",", ":")))
    if not initialized or exit_code != 0 or (args.computer_use and not (turn_completed and tool_evidence)):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
