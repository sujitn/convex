//! Actual advertised MCP handler retains the shared raw native JSON contract.
#[path = "../../../tests/fixtures/library/contract.rs"]
mod contract;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[tokio::test]
async fn mcp_library_preserves_complete_native_outputs_and_typed_errors() {
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
    let schema = serde_json::to_value(server.library_schema().await.unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(schema["content"][0]["text"].as_str().unwrap())
            .unwrap(),
        convex::library::request_schema()
    );
}
