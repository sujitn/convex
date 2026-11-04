//! Actual MCP shared handler term coupon option parity.
#[path = "../../../tests/fixtures/library/coupon_parameter_directional_contract.rs"]
mod contract;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[tokio::test]
async fn mcp_coupon_parameter_directional_preserves_native_quotes_risk_and_errors() {
    let server = ConvexMcpServer::new();
    for case in contract::cases() {
        let result = server
            .library(Parameters(LibraryToolParams {
                request_json: contract::request(&case),
            }))
            .await
            .unwrap();
        let wire = serde_json::to_value(result).unwrap();
        contract::check(&case, wire["content"][0]["text"].as_str().unwrap());
    }
}
