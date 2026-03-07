#!/usr/bin/env python3
import json
import os
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


PRIMARY_BASE_URL = env("MGS_DUAL_INSTANCE_PRIMARY_BASE_URL", "http://127.0.0.1:19180").rstrip("/")
SECONDARY_BASE_URL = env("MGS_DUAL_INSTANCE_SECONDARY_BASE_URL", "http://127.0.0.1:19181").rstrip("/")
PHONE_NUMBER = env("MGS_DUAL_INSTANCE_PHONE", "+15555550141")
SMS_CAPTURE_PATH = Path(env("MGS_DUAL_INSTANCE_SMS_CAPTURE_PATH"))
STATE_PATH = Path(env("MGS_DUAL_INSTANCE_STATE_PATH", "/tmp/mgs_dual_instance_state.json"))
MODE = env("MGS_DUAL_INSTANCE_MODE", "roundtrip")


def http_json(
    base_url: str,
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

    request = urllib.request.Request(
        f"{base_url}{path}",
        data=data,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body)
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8")
        parsed = json.loads(body) if body else {}
        return error.code, parsed


def wait_until_ready(base_url: str) -> None:
    deadline = time.time() + 60
    while time.time() < deadline:
        try:
            status, payload = http_json(base_url, "GET", "/readyz")
            if status == 200 and payload.get("ok") is True:
                return
        except Exception:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"server did not become ready at {base_url}")


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


def assert_ok(status: int, payload: dict, context: str) -> dict:
    if status != 200 or payload.get("ok") is not True:
        raise RuntimeError(f"{context} failed: status={status} payload={payload}")
    return payload["data"]


def request_and_verify(base_url: str, context_prefix: str) -> tuple[str, str]:
    SMS_CAPTURE_PATH.unlink(missing_ok=True)
    assert_ok(
        *http_json(
            base_url,
            "POST",
            "/auth/phone/request-code",
            payload={"phone_number": PHONE_NUMBER},
        ),
        context=f"request-code on {context_prefix}",
    )
    otp_code = wait_for_sms_code()
    verify_data = assert_ok(
        *http_json(
            base_url,
            "POST",
            "/auth/phone/verify-code",
            payload={"phone_number": PHONE_NUMBER, "code": otp_code},
        ),
        context=f"verify-code on {context_prefix}",
    )
    return verify_data["token"], verify_data["profile"]["user_id"]


def run_bootstrap() -> int:
    wait_until_ready(PRIMARY_BASE_URL)
    token, user_id = request_and_verify(PRIMARY_BASE_URL, "primary")
    assert_ok(
        *http_json(PRIMARY_BASE_URL, "GET", "/auth/me", token=token),
        context="auth/me on primary",
    )
    STATE_PATH.write_text(
        json.dumps({"user_id": user_id}, indent=2),
        encoding="utf-8",
    )
    print("Dual-instance bootstrap passed")
    return 0


def run_verify_secondary() -> int:
    wait_until_ready(SECONDARY_BASE_URL)
    if not STATE_PATH.exists():
        raise RuntimeError(
            f"missing persisted bootstrap state at {STATE_PATH}"
        )
    state = json.loads(STATE_PATH.read_text(encoding="utf-8"))
    expected_user_id = state["user_id"]

    token, user_id = request_and_verify(SECONDARY_BASE_URL, "secondary")
    secondary_me = assert_ok(
        *http_json(SECONDARY_BASE_URL, "GET", "/auth/me", token=token),
        context="auth/me on secondary",
    )
    if user_id != expected_user_id or secondary_me["profile"]["user_id"] != expected_user_id:
        raise RuntimeError(
            f"expected secondary instance to reuse persisted user_id {expected_user_id}, got {user_id}"
        )

    print("Dual-instance scaling smoke passed")
    return 0


def main() -> int:
    if MODE == "bootstrap":
        return run_bootstrap()
    if MODE == "verify-secondary":
        return run_verify_secondary()
    raise RuntimeError(f"unsupported dual-instance mode: {MODE}")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"dual_instance_smoke.py: {error}", file=sys.stderr)
        raise
