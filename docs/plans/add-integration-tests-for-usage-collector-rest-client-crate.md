# Add integration tests for usage-collector-rest-client crate

**Type**: implement | **Phases**: 2

**Scope**: Expose Public API + Common Helpers + REST Client Integration Tests, Delivery Pipeline Integration Tests + Build Verification

## Validation Commands

No validation commands defined.

### Task 1: Expose Public API + Common Helpers + REST Client Integration Tests

**Original Phase File:**
- `.plans/implement-rest-client-integration-tests/phase-01-rest-client-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- This phase exposes `UsageCollectorRestClientConfig` and `UsageCollectorRestClient` from the crate root via `pub use` in `src/lib.rs`, adds `tokio` `macros` and `rt-multi-thread` features to `[dev-dependencies]` in `Cargo.toml`, creates a `tests/common/mod.rs` with a `MockAuthN` helper and test-fixture functions, and creates `tests/rest_client_tests.rs` with async integration tests covering both `create_usage_record` and `get_module_config` behaviors using `httpmock` HTTP stubs. No new modules or crate dependencies are added; only the four output files are modified or created.
- **Read source files**: Read the following files from the project root to understand the current state:
- **Update `src/lib.rs`**: Add `pub use` re-exports so the two public types are accessible from the crate root. Append after the existing `mod` declarations:
- **Update `Cargo.toml`**: Replace the `tokio = { workspace = true }` entry in `[dev-dependencies]` with:
- **Create `tests/common/mod.rs`**: Create the file with the `MockAuthN` enum, its constructor methods, its `AuthNResolverClient` implementation, and the three test-fixture functions (`test_cfg`, `test_record`, `make_client`). The file must begin with `#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]`. Import `UsageCollectorRestClientConfig` and `UsageCollectorRestClient` from the crate root. Import `AuthNResolverClient`, `AuthNResolverError`, `ClientCredentialsRequest` from `authn_resolver_sdk`. Import `SecurityContext` and `BearerToken` (or the relevant type) from `modkit_security` (check the import path; `SecurityContext` is in `modkit_security::context`). Import `UsageRecord` from `usage_collector_sdk::models`. Build `SecurityContext` with a bearer token for `WithToken` using the available constructor (inspect `modkit-security` API if needed). For `exchange_client_credentials`, the return type is `Result<authn_resolver_sdk::AuthNResponse, AuthNResolverError>` — build a minimal `AuthNResponse` with the `security_context` field set
- **Create `tests/rest_client_tests.rs`**: Create the file with all 18 integration test functions listed in the Input section. Begin with `#![allow(clippy::unwrap_used, clippy::expect_used)]` and `mod common;`. Use `httpmock::MockServer::start()` in each test. Call `mock.assert()` in tests that verify request headers or body. Use the `common::` helpers for client and record construction. Pattern-match errors with `assert!(matches!(err, ...))`
- **Self-verify**: Confirm:

**Success Checks:**
- `src/lib.rs` contains `pub use config::UsageCollectorRestClientConfig;` and `pub use infra::UsageCollectorRestClient;`.
- `Cargo.toml` `[dev-dependencies]` entry for `tokio` includes `features = ["macros", "rt-multi-thread"]`.
- `tests/common/mod.rs` exists and begins with `#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]`.
- `tests/common/mod.rs` defines `MockAuthN` enum with exactly the variants `WithToken(String)`, `WithoutToken`, `Unauthorized`, `NoPlugin`.
- `tests/common/mod.rs` implements `AuthNResolverClient` for `MockAuthN` (with `exchange_client_credentials` and `authenticate`).
- `tests/common/mod.rs` exports `test_cfg`, `test_record`, and `make_client` functions.
- `tests/rest_client_tests.rs` exists and begins with `#![allow(clippy::unwrap_used, clippy::expect_used)]` and `mod common;`.
- `tests/rest_client_tests.rs` contains exactly 18 `#[tokio::test] async fn` test functions covering both `create_usage_record` and `get_module_config` behaviors.
- Every test uses `httpmock::MockServer::start()` — no real network calls.
- Tests verifying request shape call `mock.assert()`.
- Error-type tests use `assert!(matches!(err, UsageCollectorError::VariantName { .. }))`.
- `UsageCollectorRestClient` and `UsageCollectorRestClientConfig` are imported from the crate root in all test files.
- `UsageCollectorClientV1` is imported from `usage_collector_sdk` in test files, not from the crate.
- No unresolved `{...}` variables outside code fences in any created or modified file.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector-rest-client/src/lib.rs`
- Input file: `modules/system/usage-collector/usage-collector-rest-client/src/config.rs`
- Input file: `modules/system/usage-collector/usage-collector-rest-client/src/infra/mod.rs`
- Input file: `modules/system/usage-collector/usage-collector-rest-client/src/infra/rest_client.rs`
- Input file: `modules/system/usage-collector/usage-collector-rest-client/Cargo.toml`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/src/lib.rs`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/Cargo.toml`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/tests/common/mod.rs`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/tests/rest_client_tests.rs`

### Task 2: Delivery Pipeline Integration Tests + Build Verification

**Original Phase File:**
- `.plans/implement-rest-client-integration-tests/phase-02-delivery-pipeline-tests.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- This phase adds delivery-pipeline integration tests for the `cf-usage-collector-rest-client` crate. It creates `tests/delivery_pipeline_tests.rs` with six `async` tests that wire a real `UsageCollectorRestClient` into `DeliveryHandler` against a live `httpmock` server, covering success, auth-header forwarding, 500/429 retry paths, 401 reject, and AuthN-failure reject. It also updates `Cargo.toml` if the required dev-dependencies (`usage-emitter`, `modkit-db` with `sqlite` feature) are not yet present. After writing the file the phase runs the full test suite and verifies all tests pass.
- **Read runtime files.**
- **Update `Cargo.toml` dev-dependencies if needed.**
- **Create `tests/delivery_pipeline_tests.rs`.**
- **Run the full test suite.**
- **Self-verify against acceptance criteria.**

**Success Checks:**
- `tests/delivery_pipeline_tests.rs` exists under
- The file begins with `#![allow(clippy::unwrap_used, clippy::expect_used)]`
- `mod common;` appears before any `use` declarations.
- All 6 required test functions are present and annotated `#[tokio::test]`.
- Each test function is `async fn`.
- `DeliveryHandler` is imported from `usage_emitter`.
- `OutboxMessage`, `MessageResult`, and `LeasedMessageHandler` are imported
- `UsageCollectorRestClient` and `UsageCollectorRestClientConfig` are
- `make_outbox_msg` serializes with `serde_json::to_vec`.
- `Cargo.toml` `[dev-dependencies]` contains `usage-emitter = { workspace = true }`
- `cargo test -p cf-usage-collector-rest-client` exits with `test result: ok`
- No unresolved `{...}` variables outside code fences in any file written

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector-rest-client/Cargo.toml`
- Input file: `modules/system/usage-collector/usage-collector-rest-client/tests/common/mod.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/infra/delivery_handler.rs`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/tests/delivery_pipeline_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector-rest-client/Cargo.toml`
