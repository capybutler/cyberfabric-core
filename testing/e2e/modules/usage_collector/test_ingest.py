"""E2e ingestion tests for the usage-collector module.

Covers the full async delivery path: local ingest, local idempotency deduplication,
remote federation ingest (emitter → gateway), and remote idempotency deduplication
via the timescaledb plugin.
"""

from __future__ import annotations

import asyncio
import uuid
from datetime import datetime, timedelta, timezone

import pytest

from .helpers import encode_dt, wait_for_record


MODULE = "e2e-test"
RESOURCE_TYPE = "e2e.resource"
TENANT_ID = "00000000-0000-0000-0000-000000000001"


@pytest.mark.asyncio
async def test_local_ingest(gateway_client):
    """POST directly to gateway; record must appear in GET /raw on the gateway."""
    resource_id = str(uuid.uuid4())
    from_dt = datetime.now(timezone.utc)
    to_dt = from_dt + timedelta(minutes=5)

    resp = await gateway_client.post(
        "/usage-collector/v1/records",
        json={
            "module": MODULE,
            "tenant_id": TENANT_ID,
            "resource_type": RESOURCE_TYPE,
            "resource_id": resource_id,
            "metric": "e2e.gauge",
            "value": 1.0,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
    )
    assert resp.status_code == 204, f"expected 204, got {resp.status_code}: {resp.text}"

    record = await wait_for_record(gateway_client, from_dt, to_dt, resource_id=resource_id)
    assert str(record.get("resource_id")) == resource_id, (
        f"returned record resource_id mismatch: {record}"
    )


@pytest.mark.asyncio
async def test_local_ingest_idempotency(gateway_client):
    """Two POSTs with the same idempotency_key to gateway must yield exactly one record."""
    resource_id = str(uuid.uuid4())
    idempotency_key = str(uuid.uuid4())
    from_dt = datetime.now(timezone.utc)
    to_dt = from_dt + timedelta(minutes=5)

    payload = {
        "module": MODULE,
        "tenant_id": TENANT_ID,
        "resource_type": RESOURCE_TYPE,
        "resource_id": resource_id,
        "metric": "e2e.counter",
        "value": 1.0,
        "idempotency_key": idempotency_key,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }

    resp1 = await gateway_client.post("/usage-collector/v1/records", json=payload)
    assert resp1.status_code == 204, f"first POST expected 204, got {resp1.status_code}: {resp1.text}"

    resp2 = await gateway_client.post("/usage-collector/v1/records", json=payload)
    assert resp2.status_code == 204, f"second POST expected 204, got {resp2.status_code}: {resp2.text}"

    await wait_for_record(gateway_client, from_dt, to_dt, resource_id=resource_id)

    raw_resp = await gateway_client.get(
        "/usage-collector/v1/raw",
        params={"from": encode_dt(from_dt), "to": encode_dt(to_dt)},
    )
    raw_resp.raise_for_status()
    matching = [
        item for item in raw_resp.json().get("items", [])
        if str(item.get("resource_id")) == resource_id
    ]
    assert len(matching) == 1, (
        f"expected exactly 1 deduplicated record for resource_id {resource_id}, got {len(matching)}"
    )


@pytest.mark.asyncio
async def test_remote_ingest_federation(gateway_client, emitter_client):
    """POST to emitter; record must appear in GET /raw on the gateway via async federation."""
    resource_id = str(uuid.uuid4())
    from_dt = datetime.now(timezone.utc)
    to_dt = from_dt + timedelta(minutes=5)

    resp = await emitter_client.post(
        "/usage-collector/v1/records",
        json={
            "module": MODULE,
            "tenant_id": TENANT_ID,
            "resource_type": RESOURCE_TYPE,
            "resource_id": resource_id,
            "metric": "e2e.gauge",
            "value": 2.0,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
    )
    assert resp.status_code == 204, f"expected 204, got {resp.status_code}: {resp.text}"

    record = await wait_for_record(gateway_client, from_dt, to_dt, resource_id=resource_id)
    assert str(record.get("resource_id")) == resource_id, (
        f"federated record not found on gateway: {record}"
    )


@pytest.mark.asyncio
async def test_remote_ingest_idempotency(gateway_client, emitter_client):
    """Two POSTs with the same idempotency_key to emitter; plugin deduplicates at gateway."""
    resource_id = str(uuid.uuid4())
    idempotency_key = str(uuid.uuid4())
    from_dt = datetime.now(timezone.utc)
    to_dt = from_dt + timedelta(minutes=5)

    payload = {
        "module": MODULE,
        "tenant_id": TENANT_ID,
        "resource_type": RESOURCE_TYPE,
        "resource_id": resource_id,
        "metric": "e2e.counter",
        "value": 1.0,
        "idempotency_key": idempotency_key,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }

    resp1 = await emitter_client.post("/usage-collector/v1/records", json=payload)
    assert resp1.status_code == 204, f"first POST expected 204, got {resp1.status_code}: {resp1.text}"

    resp2 = await emitter_client.post("/usage-collector/v1/records", json=payload)
    assert resp2.status_code == 204, f"second POST expected 204, got {resp2.status_code}: {resp2.text}"

    await wait_for_record(gateway_client, from_dt, to_dt, resource_id=resource_id)
    # Poll for several seconds to confirm no duplicate arrives — both deliveries are async
    # so the second outbox flush may not have reached the gateway yet when wait_for_record returns.
    loop = asyncio.get_running_loop()
    stable_deadline = loop.time() + 5.0
    matching = []
    while loop.time() < stable_deadline:
        await asyncio.sleep(0.5)
        raw_resp = await gateway_client.get(
            "/usage-collector/v1/raw",
            params={"from": encode_dt(from_dt), "to": encode_dt(to_dt)},
        )
        raw_resp.raise_for_status()
        matching = [
            item for item in raw_resp.json().get("items", [])
            if str(item.get("resource_id")) == resource_id
        ]
        if len(matching) > 1:
            break

    assert len(matching) == 1, (
        f"expected exactly 1 deduplicated record for resource_id {resource_id}, got {len(matching)}"
    )
