//! Actual shared MCP handler over the original native delivery-basket owners.
#[path = "../../../tests/fixtures/library/delivery_hedge_contract.rs"]
mod controls;
use convex_mcp::server::{ConvexMcpServer, LibraryToolParams};
use rmcp::handler::server::wrapper::Parameters;
#[test]
fn mcp_delivery_basket_columns_and_book() {
    let server = ConvexMcpServer::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    controls::run(|request| {
        let response = runtime
            .block_on(server.library(Parameters(LibraryToolParams {
                request_json: request.to_owned(),
            })))
            .unwrap();
        serde_json::to_value(response).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    });
}
