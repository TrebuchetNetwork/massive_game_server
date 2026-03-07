#!/usr/bin/env python3
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional


def env(name: str, default: Optional[str] = None) -> str:
    value = os.environ.get(name, default)
    if value is None:
        raise RuntimeError(f"missing required environment variable: {name}")
    return value


BASE_URL = env("MGS_REDIS_FAILOVER_BASE_URL", "http://127.0.0.1:19081").rstrip("/")
PHONE_NUMBER = env("MGS_REDIS_FAILOVER_PHONE", "+15555550121")
SMS_CAPTURE_PATH = Path(env("MGS_REDIS_FAILOVER_SMS_CAPTURE_PATH"))
AUTH_STORE_PATH = Path(env("MGS_REDIS_FAILOVER_AUTH_STORE_PATH"))
REDIS_CONTAINER = env("MGS_REDIS_FAILOVER_CONTAINER", "mgs-redis-failover")
REDIS_PASSWORD = env("MGS_REDIS_FAILOVER_PASSWORD", "mgs_redis_dev_password")
REDIS_STORE_KEY = env("MGS_REDIS_FAILOVER_STORE_KEY", "mgs:auth:persistent_store")


def http_json(
    method: str,
    path: str,
    token: Optional[str] = None,
    payload: Optional[dict] = None,
) -> tuple[int, dict]:
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"

    request = urllib.request.Request(f"{BASE_URL}{path}", data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body)
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8")
        parsed = json.loads(body) if body else {}
        return error.code, parsed


def wait_until_ready() -> None:
    deadline = time.time() + 60
    while time.time() < deadline:
        try:
            status, payload = http_json("GET", "/readyz")
            if status == 200 and payload.get("ok") is True:
                return
        except Exception:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"server did not become ready at {BASE_URL}")


def wait_for_sms_code() -> str:
    deadline = time.time() + 30
    while time.time() < deadline:
        if SMS_CAPTURE_PATH.exists():
            raw = SMS_CAPTURE_PATH.read_text(encoding="utf-8")
            for token in raw.split():
                digits = "".join(ch for ch in token if ch.isdigit())
                if len(digits) == 6:
                    return digits
        time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for OTP in {SMS_CAPTURE_PATH}")


def redis_cli(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    command = [
        "docker",
        "exec",
        REDIS_CONTAINER,
        "redis-cli",
        "--raw",
        "-a",
        REDIS_PASSWORD,
        *args,
    ]
    return subprocess.run(command, check=check, capture_output=True, text=True)


def wait_for_redis_ready() -> None:
    deadline = time.time() + 30
    while time.time() < deadline:
        result = redis_cli("PING", check=False)
        if result.returncode == 0 and "PONG" in result.stdout:
            return
        time.sleep(0.5)
    raise RuntimeError("redis did not recover in time")


def redis_store_json() -> Optional[dict]:
    result = redis_cli("GET", REDIS_STORE_KEY, check=False)
    if result.returncode != 0:
        return None
    payload = result.stdout.strip()
    if not payload or payload == "(nil)":
        return None
    return json.loads(payload)


def wait_for_redis_store(predicate, description: str) -> dict:
    deadline = time.time() + 30
    while time.time() < deadline:
        store = redis_store_json()
        if store is not None and predicate(store):
            return store
        time.sleep(0.5)
    raise RuntimeError(f"timed out waiting for redis store state: {description}")


def wait_for_file_store(predicate, description: str) -> dict:
    deadline = time.time() + 30
    while time.time() < deadline:
        if AUTH_STORE_PATH.exists():
            store = json.loads(AUTH_STORE_PATH.read_text(encoding="utf-8"))
            if predicate(store):
                return store
        time.sleep(0.25)
    raise RuntimeError(f"timed out waiting for file store state: {description}")


def assert_ok(status: int, payload: dict, context: str) -> dict:
    if status != 200 or payload.get("ok") is not True:
        raise RuntimeError(f"{context} failed: status={status} payload={payload}")
    return payload["data"]


def main() -> int:
    wait_until_ready()
    SMS_CAPTURE_PATH.unlink(missing_ok=True)

    assert_ok(
        *http_json("POST", "/auth/phone/request-code", payload={"phone_number": PHONE_NUMBER}),
        context="request-code",
    )
    otp_code = wait_for_sms_code()
    verify_data = assert_ok(
        *http_json(
            "POST",
            "/auth/phone/verify-code",
            payload={"phone_number": PHONE_NUMBER, "code": otp_code},
        ),
        context="verify-code",
    )
    token = verify_data["token"]
    user_id = verify_data["profile"]["user_id"]

    wait_for_redis_store(
        lambda store: user_id in store.get("users", {}),
        "initial user persisted to redis",
    )

    subprocess.run(["docker", "stop", REDIS_CONTAINER], check=True, capture_output=True, text=True)

    assert_ok(*http_json("GET", "/auth/me", token=token), context="auth/me while redis down")
    assert_ok(
        *http_json("POST", "/auth/delete-account", token=token),
        context="delete-account while redis down",
    )
    wait_for_file_store(
        lambda store: user_id in store.get("pending_deletions", {}),
        "pending deletion persisted to file while redis down",
    )

    subprocess.run(["docker", "start", REDIS_CONTAINER], check=True, capture_output=True, text=True)
    wait_for_redis_ready()

    assert_ok(
        *http_json("POST", "/auth/cancel-deletion", token=token),
        context="cancel-deletion after redis restart",
    )
    wait_for_redis_store(
        lambda store: user_id in store.get("users", {})
        and user_id not in store.get("pending_deletions", {}),
        "redis store resynced after recovery",
    )
    assert_ok(*http_json("GET", "/auth/me", token=token), context="auth/me after redis recovery")

    print("Redis failover smoke passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"redis_failover_smoke.py: {error}", file=sys.stderr)
        raise
