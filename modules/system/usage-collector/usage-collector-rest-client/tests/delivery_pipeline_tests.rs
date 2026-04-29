#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use chrono::Utc;
use httpmock::prelude::*;
use modkit_db::outbox::{LeasedMessageHandler, MessageResult, OutboxMessage};
use usage_collector_rest_client::UsageCollectorRestClient;
use usage_collector_sdk::models::UsageRecord;
use usage_emitter::DeliveryHandler;

use common::{MockAuthN, make_client, test_record};

fn make_outbox_msg(record: &UsageRecord) -> OutboxMessage {
    OutboxMessage {
        partition_id: 0,
        seq: 1,
        payload: serde_json::to_vec(record).unwrap(),
        payload_type: "application/json".to_owned(),
        created_at: Utc::now(),
        attempts: 0,
    }
}

#[tokio::test]
async fn delivery_handler_with_rest_client_succeeds_on_204() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let client: UsageCollectorRestClient =
        make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    assert!(matches!(result, MessageResult::Ok));
}

#[tokio::test]
async fn delivery_handler_sends_authorization_header_to_collector() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/usage-collector/v1/records")
            .header("authorization", "Bearer test-token");
        then.status(204);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("test-token"));
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    mock.assert();
    assert!(matches!(result, MessageResult::Ok));
}

#[tokio::test]
async fn delivery_handler_with_rest_client_retries_on_server_500() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(500);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn delivery_handler_with_rest_client_retries_on_server_429() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(429);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn delivery_handler_with_rest_client_rejects_on_server_401() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}

#[tokio::test]
async fn delivery_handler_with_rest_client_rejects_on_authn_failure() {
    let client = make_client("http://localhost:1", MockAuthN::unauthorized());
    let handler = DeliveryHandler::new(Arc::new(client));
    let record = test_record();
    let msg = make_outbox_msg(&record);
    let result = handler.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}
