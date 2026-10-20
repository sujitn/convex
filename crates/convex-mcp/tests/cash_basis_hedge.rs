//! Actual MCP shared handler; financial control comparisons remain independent.
#[path = "../../../tests/fixtures/library/cash_basis_hedge_contract.rs"]
mod controls;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
async fn execute(server: &ConvexMcpServer, case: &serde_json::Value) -> String {
    let output = server
        .library(Parameters(LibraryToolParams {
            request_json: controls::request(case),
        }))
        .await
        .unwrap();
    serde_json::to_value(output).unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn mcp_cash_basis_hedge_and_complete_hedge_columns_preserve_original_controls() {
    let server = ConvexMcpServer::new();
    let mut outputs = Vec::new();
    for case in controls::cases() {
        let output = execute(&server, &case).await;
        controls::check(&case, &output);
        outputs.push(serde_json::json!({"id":case["id"],"expected_envelope":serde_json::from_str::<serde_json::Value>(&output).unwrap()}));
        for owner in controls::owner_requests(&case, &output) {
            controls::check_owner(&owner, &execute(&server, &owner).await);
        }
    }
    for case in controls::mixed_requests(&outputs) {
        controls::check_mixed(&case, &execute(&server, &case).await);
    }
}
