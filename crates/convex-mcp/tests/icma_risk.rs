//! Actual MCP shared handler; financial control comparisons remain independent.
#[path = "../../../tests/fixtures/library/icma_risk_contract.rs"]
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
async fn mcp_icma_risk_and_complete_hedge_columns_preserve_original_controls() {
    let server = ConvexMcpServer::new();
    for case in controls::cases() {
        let output = execute(&server, &case).await;
        controls::check(&case, &output);
        if case["expected_error"].is_null()
            && case["request"]["operation"]["action"]["kind"] == "hedge_vector"
        {
            for hedge in controls::hedge_requests(&case, &output) {
                controls::check_hedge(&hedge, &execute(&server, &hedge).await);
            }
        }
    }
}
