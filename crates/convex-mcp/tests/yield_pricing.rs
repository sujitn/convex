//! Actual MCP shared supplied_cash handler parity.
#[path = "../../../tests/fixtures/library/yield_pricing_contract.rs"]
mod contract;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[tokio::test]
async fn mcp_yield_pricing_preserves_native_values_units_and_errors() {
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
