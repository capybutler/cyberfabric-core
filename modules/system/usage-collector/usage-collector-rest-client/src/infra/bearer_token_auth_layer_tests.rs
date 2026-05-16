#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use authn_resolver_sdk::{AuthNResolverError, ClientCredentialsRequest};
use bytes::Bytes;
use http::header::AUTHORIZATION;
use http::{HeaderValue, Request, Response, StatusCode};
use http_body_util::Full;
use modkit_http::HttpError;
use tower::{Layer, Service};

use super::{BearerTokenAuthLayer, BearerTokenAuthService};
use super::super::test_support::MockAuthN;

fn make_creds() -> ClientCredentialsRequest {
    ClientCredentialsRequest {
        client_id: "test-client".to_owned(),
        client_secret: "test-secret".to_owned().into(),
        scopes: vec![],
    }
}

#[derive(Clone)]
struct CapturingService {
    captured: Arc<Mutex<Option<HeaderValue>>>,
}

impl Service<Request<Full<Bytes>>> for CapturingService {
    type Response = Response<Full<Bytes>>;
    type Error = HttpError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Full<Bytes>>) -> Self::Future {
        let captured = Arc::clone(&self.captured);
        let hv = req.headers().get(AUTHORIZATION).cloned();
        Box::pin(async move {
            *captured.lock().unwrap() = hv;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::new()))
                .unwrap())
        })
    }
}

#[tokio::test]
async fn injects_authorization_header() {
    let captured = Arc::new(Mutex::new(None));
    let inner = CapturingService { captured: Arc::clone(&captured) };
    let layer = BearerTokenAuthLayer::new(MockAuthN::with_token("test-bearer-token"), make_creds());
    let mut svc = layer.layer(inner);

    let req = Request::builder()
        .uri("http://example.com/test")
        .body(Full::new(Bytes::new()))
        .unwrap();

    svc.call(req).await.unwrap();

    let hv = captured.lock().unwrap().clone().expect("Authorization header not set");
    assert_eq!(hv.to_str().unwrap(), "Bearer test-bearer-token");
}

#[tokio::test]
async fn header_value_is_sensitive() {
    let captured = Arc::new(Mutex::new(None));
    let inner = CapturingService { captured: Arc::clone(&captured) };
    let layer = BearerTokenAuthLayer::new(MockAuthN::with_token("sensitive-token"), make_creds());
    let mut svc = layer.layer(inner);

    let req = Request::builder()
        .uri("http://example.com/test")
        .body(Full::new(Bytes::new()))
        .unwrap();

    svc.call(req).await.unwrap();

    let hv = captured.lock().unwrap().clone().expect("Authorization header not set");
    assert!(hv.is_sensitive(), "Authorization header value must be marked sensitive");
}

#[tokio::test]
async fn authn_error_propagates_as_http_transport_error() {
    let captured = Arc::new(Mutex::new(None));
    let inner = CapturingService { captured };
    let layer = BearerTokenAuthLayer::new(MockAuthN::unauthorized(), make_creds());
    let mut svc = layer.layer(inner);

    let req = Request::builder()
        .uri("http://example.com/test")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let err = svc.call(req).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Transport(_)),
        "expected Transport error, got: {err:?}"
    );
}

#[tokio::test]
async fn authn_error_propagates_and_downcasts_as_auth_n_resolver_error() {
    let captured = Arc::new(Mutex::new(None));
    let inner = CapturingService { captured };
    let layer = BearerTokenAuthLayer::new(MockAuthN::unauthorized(), make_creds());
    let mut svc = layer.layer(inner);

    let req = Request::builder()
        .uri("http://example.com/test")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let err = svc.call(req).await.unwrap_err();
    if let HttpError::Transport(boxed) = err {
        let auth_err = boxed
            .downcast::<AuthNResolverError>()
            .expect("should downcast to AuthNResolverError");
        assert!(
            matches!(*auth_err, AuthNResolverError::Unauthorized(_)),
            "expected Unauthorized variant, got: {:?}",
            *auth_err
        );
    } else {
        panic!("expected Transport error");
    }
}

#[tokio::test]
async fn missing_bearer_token_returns_transport_error_with_internal_variant() {
    let captured = Arc::new(Mutex::new(None));
    let inner = CapturingService { captured };
    let layer = BearerTokenAuthLayer::new(MockAuthN::without_token(), make_creds());
    let mut svc = layer.layer(inner);

    let req = Request::builder()
        .uri("http://example.com/test")
        .body(Full::new(Bytes::new()))
        .unwrap();

    let err = svc.call(req).await.unwrap_err();
    if let HttpError::Transport(boxed) = err {
        let auth_err = boxed
            .downcast::<AuthNResolverError>()
            .expect("should downcast to AuthNResolverError");
        assert!(
            matches!(*auth_err, AuthNResolverError::Internal(_)),
            "expected Internal variant, got: {:?}",
            *auth_err
        );
    } else {
        panic!("expected Transport error, got: {err:?}");
    }
}

#[test]
fn layer_is_clone_send_sync() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<BearerTokenAuthLayer>();
    assert_traits::<BearerTokenAuthService<CapturingService>>();
}
