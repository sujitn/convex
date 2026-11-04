#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod fixture;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[tokio::test]
async fn actual_mcp_full_product_calibration_results_and_failures_match_native() {
    let server = ConvexMcpServer::new();
    for case in fixture::transport_cases() {
        let output = server
            .library(Parameters(LibraryToolParams {
                request_json: fixture::request(&case, serde_json::json!({"kind":"build"})),
            }))
            .await
            .unwrap();
        let wire = serde_json::to_value(output).unwrap();
        fixture::check_transport(&case, wire["content"][0]["text"].as_str().unwrap());
    }
}
