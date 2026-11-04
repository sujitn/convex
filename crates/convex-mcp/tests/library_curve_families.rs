//! Actual MCP shared handler curve_families parity.
#[path = "../../../tests/fixtures/library/curve_family_contract.rs"]
mod contract;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[tokio::test]
async fn mcp_curve_families_preserve_native_curve_families_and_errors() {
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
