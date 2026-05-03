"""Usage-collector e2e conftest — two-server federation topology.

Instance 2 (gateway): hyperspot-server with usage-collector + timescaledb plugin.
Instance 1 (emitter): hyperspot-server with usage-collector + usage-collector-rest-client.

Binary selection:
  E2E_BINARY        — path to the gateway binary (required for both if E2E_BINARY_EMITTER is absent)
  E2E_BINARY_EMITTER — path to the emitter binary; falls back to E2E_BINARY if unset
"""

from __future__ import annotations

import asyncio
import os
import socket
import subprocess
import tempfile
import time
from pathlib import Path

import httpx
import pytest

from lib.orchestrator import ModuleTestEnv, RunningTestEnv


MODULE_DIR = Path(__file__).resolve().parent
PROJECT_ROOT = MODULE_DIR.parents[3]  # testing/e2e/modules/usage_collector -> repo root
_BASE_CONFIG = MODULE_DIR / "config" / "base.yaml"


# ── Port allocation ───────────────────────────────────────────────────────────

def _two_free_ports() -> tuple[int, int]:
    """Return two distinct OS-assigned free ports, allocated simultaneously."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s1, \
         socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s2:
        s1.bind(("", 0))
        s2.bind(("", 0))
        return s1.getsockname()[1], s2.getsockname()[1]


_GATEWAY_PORT, _EMITTER_PORT = _two_free_ports()


# ── Sidecar ───────────────────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def timescaledb_sidecar():
    """Start and stop the TimescaleDB Docker sidecar for the full test session."""
    from .timescaledb_sidecar import TimescaleDbSidecar
    sidecar = TimescaleDbSidecar()
    sidecar.start()
    yield sidecar
    sidecar.stop()


# ── Config-patch helpers ──────────────────────────────────────────────────────

def _patch_gateway_config(config_text: str, sidecar, port: int) -> str:
    config_text = config_text.replace("__DB_URL__", sidecar.connection_string)
    config_text = config_text.replace("__PORT__", str(port))
    return config_text


def _patch_emitter_config(config_text: str, gateway_url: str, port: int) -> str:
    config_text = config_text.replace("__COLLECTOR_URL__", gateway_url)
    config_text = config_text.replace("__PORT__", str(port))
    config_text = config_text.replace("__DB_URL__", "disabled")
    return config_text


# ── ModuleTestEnv fixtures ────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def module_test_env(timescaledb_sidecar):
    """Gateway (Instance 2): usage-collector + timescaledb plugin."""
    def patch(config_text: str, env: ModuleTestEnv) -> str:
        return _patch_gateway_config(config_text, timescaledb_sidecar, _GATEWAY_PORT)

    return ModuleTestEnv(
        config_path=_BASE_CONFIG,
        config_patch=patch,
        port=_GATEWAY_PORT,
        health_path="/healthz",
        health_timeout=90,
        log_suffix="uc-gateway",
    )


@pytest.fixture(scope="session")
def emitter_test_env(test_env):
    """Emitter (Instance 1): usage-collector + usage-collector-rest-client.

    Depends on test_env so the gateway is already running and its URL is known.
    """
    gateway_url = test_env.base_url

    def patch(config_text: str, env: ModuleTestEnv) -> str:
        return _patch_emitter_config(config_text, gateway_url, _EMITTER_PORT)

    emitter_binary = os.environ.get(
        "E2E_BINARY_EMITTER",
        os.environ.get("E2E_BINARY", "hyperspot-server"),
    )

    return ModuleTestEnv(
        binary=emitter_binary,
        config_path=_BASE_CONFIG,
        config_patch=patch,
        port=_EMITTER_PORT,
        health_path="/healthz",
        health_timeout=90,
        log_suffix="uc-emitter",
    )


# ── Emitter server lifecycle ──────────────────────────────────────────────────

def _resolve_binary_path(env: ModuleTestEnv) -> Path:
    from shutil import which

    env_binary = os.environ.get("E2E_BINARY_EMITTER") or os.environ.get("E2E_BINARY")
    if env_binary:
        p = Path(env_binary)
        if p.exists():
            return p
        pytest.fail(f"Emitter binary not found: {env_binary}")

    p = Path(env.binary)
    if p.exists():
        return p

    found = which(env.binary)
    if found:
        return Path(found)

    pytest.fail(
        f"Emitter binary not found: {env.binary}\n"
        "Set E2E_BINARY_EMITTER (or E2E_BINARY) to the path of the emitter binary."
    )


@pytest.fixture(scope="session")
def emitter_env(emitter_test_env):
    """Start the emitter server (Instance 1) and yield its RunningTestEnv."""
    env = emitter_test_env

    # Prepare config
    config_text = env.config_path.read_text()
    if env.config_patch:
        config_text = env.config_patch(config_text, env)

    tmp = tempfile.NamedTemporaryFile(
        prefix="e2e-config-emitter-", suffix=".yaml", mode="w", delete=False,
    )
    tmp.write(config_text)
    tmp.close()
    config_path = Path(tmp.name)

    binary = _resolve_binary_path(env)

    logs_dir = PROJECT_ROOT / "testing" / "e2e" / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)
    log = logs_dir / f"hyperspot-e2e-{env.port}-uc-emitter.log"

    proc = None
    log_fh = None
    try:
        log_fh = open(log, "w")
        proc = subprocess.Popen(
            [str(binary), "--config", str(config_path), "run"],
            cwd=str(PROJECT_ROOT),
            stdout=log_fh,
            stderr=subprocess.STDOUT,
        )

        # Wait for health
        health_url = f"http://localhost:{env.port}{env.health_path}"
        deadline = time.monotonic() + env.health_timeout
        while time.monotonic() < deadline:
            try:
                r = httpx.get(health_url, timeout=3)
                if r.status_code == 200:
                    break
            except httpx.ConnectError:
                pass
            time.sleep(1)
        else:
            log_tail = log.read_text()[-3000:] if log.exists() else ""
            pytest.fail(
                f"Emitter server did not become healthy within {env.health_timeout}s.\n"
                f"Health URL: {health_url}\nLog tail:\n{log_tail}"
            )

        yield RunningTestEnv(
            base_url=f"http://localhost:{env.port}",
            env=env,
            sidecars={},
        )

    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    pass
        if log_fh is not None:
            log_fh.close()
        config_path.unlink(missing_ok=True)


# ── HTTP client fixtures ──────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def gateway_client(test_env):
    """Pre-configured async HTTP client targeting the gateway (Instance 2)."""
    client = httpx.AsyncClient(
        base_url=f"{test_env.base_url}/cf",
        timeout=30.0,
    )
    yield client
    asyncio.run(client.aclose())


@pytest.fixture(scope="session")
def emitter_client(emitter_env):
    """Pre-configured async HTTP client targeting the emitter (Instance 1)."""
    client = httpx.AsyncClient(
        base_url=f"{emitter_env.base_url}/cf",
        timeout=30.0,
    )
    yield client
    asyncio.run(client.aclose())
