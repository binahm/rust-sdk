#![cfg(not(feature = "local"))]
use std::time::Duration;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::{SessionId, local::LocalSessionManager},
};
use tokio_util::sync::CancellationToken;

mod common;
use common::calculator::Calculator;

#[tokio::test]
async fn test_priming_on_stream_start() -> anyhow::Result<()> {
    let ct = CancellationToken::new();

    // stateful_mode: true automatically enables priming with DEFAULT_RETRY_INTERVAL (3 seconds)
    let service: StreamableHttpService<Calculator, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(Calculator::new()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_sse_keep_alive(None)
                .with_cancellation_token(ct.child_token()),
        );

    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = tcp_listener.local_addr()?;

    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    // Send initialize request
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let body = response.text().await?;

    // Split SSE events by double newline
    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert!(events.len() >= 2);

    // Verify priming event (first event) — initialize uses "0" (no http_request_id)
    let priming_event = events[0];
    assert!(priming_event.contains("id: 0"));
    assert!(priming_event.contains("retry: 3000"));
    assert!(priming_event.contains("data:"));

    // Verify initialize response (second event)
    let response_event = events[1];
    assert!(response_event.contains(r#""jsonrpc":"2.0""#));
    assert!(response_event.contains(r#""id":1"#));

    ct.cancel();
    handle.await?;

    Ok(())
}

#[tokio::test]
async fn test_request_wise_priming_includes_http_request_id() -> anyhow::Result<()> {
    let ct = CancellationToken::new();

    let service: StreamableHttpService<Calculator, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(Calculator::new()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_sse_keep_alive(None)
                .with_cancellation_token(ct.child_token()),
        );

    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = tcp_listener.local_addr()?;

    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    let client = reqwest::Client::new();

    // Initialize the session
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let session_id: SessionId = response.headers()["mcp-session-id"].to_str()?.into();

    // Send notifications/initialized
    let status = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .send()
        .await?
        .status();
    assert_eq!(status, 202);

    // First tool call — should get http_request_id 0
    let body = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"sum","arguments":{"a":1,"b":2}}}"#)
        .send()
        .await?
        .text()
        .await?;

    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert!(
        events.len() >= 2,
        "expected priming + response, got: {body}"
    );

    // Priming event should encode the http_request_id (0)
    let priming = events[0];
    assert!(
        priming.contains("id: 0/0"),
        "first request priming should be 0/0, got: {priming}"
    );
    assert!(priming.contains("retry: 3000"));

    // Response event should use index 1 (since priming occupies index 0)
    let response_event = events[1];
    assert!(
        response_event.contains("id: 1/0"),
        "first response event id should be 1/0, got: {response_event}"
    );
    assert!(response_event.contains(r#""id":2"#));

    // Second tool call — should get http_request_id 1
    let body = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"sum","arguments":{"a":3,"b":4}}}"#)
        .send()
        .await?
        .text()
        .await?;

    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert!(
        events.len() >= 2,
        "expected priming + response, got: {body}"
    );

    let priming = events[0];
    assert!(
        priming.contains("id: 0/1"),
        "second request priming should be 0/1, got: {priming}"
    );

    let response_event = events[1];
    assert!(
        response_event.contains("id: 1/1"),
        "second response event id should be 1/1, got: {response_event}"
    );
    assert!(response_event.contains(r#""id":3"#));

    ct.cancel();
    handle.await?;

    Ok(())
}

#[tokio::test]
async fn test_resume_after_request_wise_channel_completed() -> anyhow::Result<()> {
    let ct = CancellationToken::new();

    let service: StreamableHttpService<Calculator, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(Calculator::new()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_sse_keep_alive(None)
                .with_cancellation_token(ct.child_token()),
        );

    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = tcp_listener.local_addr()?;

    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    let client = reqwest::Client::new();

    // Initialize session
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let session_id: SessionId = response.headers()["mcp-session-id"].to_str()?.into();

    // Complete handshake
    let status = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .send()
        .await?
        .status();
    assert_eq!(status, 202);

    // Call a tool and consume the full response (channel completes)
    let body = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"sum","arguments":{"a":1,"b":2}}}"#)
        .send()
        .await?
        .text()
        .await?;

    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert!(
        events.len() >= 2,
        "expected priming + response, got: {body}"
    );
    assert!(events[0].contains("id: 0/0"));
    assert!(events[1].contains(r#""id":2"#));

    // Resume with Last-Event-ID after the channel has completed.
    // The cached events should be replayed and the stream should end.
    let resume_response = client
        .get(format!("http://{addr}/mcp"))
        .header("Accept", "text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .header("Mcp-Protocol-Version", "2025-06-18")
        .header("last-event-id", "0/0")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;
    assert_eq!(resume_response.status(), 200);

    let resume_body = resume_response.text().await?;
    let resume_events: Vec<&str> = resume_body
        .split("\n\n")
        .filter(|e| !e.is_empty())
        .collect();
    assert!(
        !resume_events.is_empty(),
        "expected replayed events on resume, got empty"
    );

    // The replayed event should contain the original response
    let replayed = resume_events[0];
    assert!(
        replayed.contains(r#""id":2"#),
        "replayed event should contain the tool response, got: {replayed}"
    );

    ct.cancel();
    handle.await?;

    Ok(())
}

#[tokio::test]
async fn test_priming_on_stream_close() -> anyhow::Result<()> {
    use std::sync::Arc;

    use rmcp::transport::streamable_http_server::session::SessionId;

    let ct = CancellationToken::new();
    let session_manager = Arc::new(LocalSessionManager::default());

    // stateful_mode: true automatically enables priming with DEFAULT_RETRY_INTERVAL (3 seconds)
    let service = StreamableHttpService::new(
        || Ok(Calculator::new()),
        session_manager.clone(),
        StreamableHttpServerConfig::default()
            .with_sse_keep_alive(None)
            .with_cancellation_token(ct.child_token()),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = tcp_listener.local_addr()?;

    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    // Send initialize request to create a session
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#)
        .send()
        .await?;

    let session_id: SessionId = response.headers()["mcp-session-id"].to_str()?.into();

    // Open a standalone GET stream (send() returns when headers are received)
    let response = client
        .get(format!("http://{addr}/mcp"))
        .header("Accept", "text/event-stream")
        .header("mcp-session-id", session_id.to_string())
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    // Spawn a task to read the response body (blocks until stream closes)
    let read_task = tokio::spawn(async move { response.text().await.unwrap() });

    // Close the standalone stream with a 5-second retry hint
    let sessions = session_manager.sessions.read().await;
    let session = sessions.get(&session_id).unwrap();
    session
        .close_standalone_sse_stream(Some(Duration::from_secs(5)))
        .await?;
    drop(sessions);

    // Wait for the read task to complete and verify the response
    let body = read_task.await?;

    // Verify the stream received two priming events:
    // 1. At stream start (retry: 3000)
    // 2. Before close (retry: 5000)
    let events: Vec<&str> = body.split("\n\n").filter(|e| !e.is_empty()).collect();
    assert_eq!(events.len(), 2);

    // First event: priming at stream start
    let start_priming = events[0];
    assert!(start_priming.contains("id:"));
    assert!(start_priming.contains("retry: 3000"));
    assert!(start_priming.contains("data:"));

    // Second event: priming before close
    let close_priming = events[1];
    assert!(close_priming.contains("id:"));
    assert!(close_priming.contains("retry: 5000"));
    assert!(close_priming.contains("data:"));

    ct.cancel();
    handle.await?;

    Ok(())
}

#[cfg(test)]
mod test_priming_resume {

    use rmcp::{
        ServerHandler,
        handler::server::router::tool::ToolRouter,
        model::{CallToolResult, Content, ErrorData as McpError, ServerCapabilities, ServerInfo},
        service::{RoleClient, RunningService, serve_client},
        tool, tool_handler, tool_router,
        transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
            streamable_http_server::{
                StreamableHttpServerConfig, StreamableHttpService,
                session::local::LocalSessionManager,
            },
        },
    };
    use tokio_util::sync::CancellationToken;

    // Guard against infinite loops in any call_tool invocation.
    const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    // we use a short reqwest timeout to trigger the priming / event replay.
    // REQWEST_TIMEOUT should be shorter than the tool runtime LONG_TASK_DURATION
    const REQWEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
    const LONG_TASK_DURATION: std::time::Duration = std::time::Duration::from_secs(5);

    fn init_logging() {
        // Safe to call from multiple tests in the same process.
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "error".to_string().into()),
            )
            .with_file(true)
            .with_line_number(true)
            .try_init();
    }

    /// Spin up a `LongRunning` MCP server on a random port.
    /// Returns the bound address, a cancellation token to stop the server, and its task handle.
    async fn setup_server() -> anyhow::Result<(
        std::net::SocketAddr,
        CancellationToken,
        tokio::task::JoinHandle<()>,
    )> {
        let ct = CancellationToken::new();
        let service: StreamableHttpService<LongRunning, LocalSessionManager> =
            StreamableHttpService::new(
                || Ok(LongRunning::new()),
                Default::default(),
                StreamableHttpServerConfig::default()
                    .with_sse_keep_alive(None)
                    .with_cancellation_token(ct.child_token()),
            );
        let router = axum::Router::new().nest_service("/mcp", service);
        let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = tcp_listener.local_addr()?;
        let server_handle = tokio::spawn({
            let ct = ct.clone();
            async move {
                let _ = axum::serve(tcp_listener, router)
                    .with_graceful_shutdown(ct.cancelled_owned())
                    .await;
            }
        });
        Ok((addr, ct, server_handle))
    }

    /// Connect an MCP client to `addr`.
    /// Uses a short request timeout (3 s, below the tool's 5 s runtime) to trigger
    /// the priming / event-replay code path.
    async fn setup_client(
        addr: std::net::SocketAddr,
    ) -> anyhow::Result<RunningService<RoleClient, ()>> {
        let reqwest_client = reqwest::Client::builder()
            .timeout(REQWEST_TIMEOUT)
            .connection_verbose(true)
            .build()?;
        let transport = StreamableHttpClientTransport::with_client(
            reqwest_client,
            StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")),
        );
        Ok(serve_client((), transport).await?)
    }

    fn assert_tool_success(label: &str, result: &CallToolResult) {
        assert!(
            result.is_error != Some(true),
            "{label} call_tool expected success, got: {result:?}"
        );
        assert_eq!(
            result.content.len(),
            1,
            "{label} call_tool expected 1 content item"
        );
        assert_eq!(
            result.content[0].as_text().unwrap().text,
            "Long task completed"
        );
    }

    #[derive(Debug, Clone, Default)]
    pub struct LongRunning {
        tool_router: ToolRouter<Self>,
    }

    impl LongRunning {
        pub fn new() -> Self {
            Self {
                tool_router: Self::tool_router(),
            }
        }
    }

    #[tool_router]
    impl LongRunning {
        #[tool(description = "Run a long running tool call")]
        async fn long_task(&self) -> Result<CallToolResult, McpError> {
            tokio::time::sleep(LONG_TASK_DURATION).await;
            Ok(CallToolResult::success(vec![Content::text(
                "Long task completed",
            )]))
        }
    }

    #[tool_handler(router = self.tool_router)]
    impl ServerHandler for LongRunning {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }
    }

    #[tokio::test]
    async fn test_long_running_tool_single_via_mcp_client() -> anyhow::Result<()> {
        init_logging();
        let (addr, ct, server_handle) = setup_server().await?;
        let client = setup_client(addr).await?;

        let result = tokio::time::timeout(
            CALL_TIMEOUT,
            client.call_tool(rmcp::model::CallToolRequestParams::new("long_task")),
        )
        .await;

        // Always clean up before asserting.
        let _ = client.cancel().await;
        ct.cancel();
        server_handle.await?;

        let result = result.expect("call_tool timed out - client may be stuck in endless loop")?;
        assert_tool_success("single", &result);
        assert_eq!(
            result.content[0].as_text().unwrap().text,
            "Long task completed"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_long_running_tool_parallel_via_mcp_client() -> anyhow::Result<()> {
        init_logging();
        let (addr, ct, server_handle) = setup_server().await?;
        let client = setup_client(addr).await?;

        // Spawn a second call delayed by 4 seconds so it overlaps with the first
        // mid-flight, exercising the priming / event-replay path.
        let parallel_handle = tokio::spawn({
            let client = client.clone();
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                client
                    .call_tool(rmcp::model::CallToolRequestParams::new("long_task"))
                    .await
            }
        });

        // Run both timeouts concurrently so the total wall time is bounded at 15 s.
        let (main_result, parallel_result) = tokio::join!(
            tokio::time::timeout(
                CALL_TIMEOUT,
                client.call_tool(rmcp::model::CallToolRequestParams::new("long_task")),
            ),
            tokio::time::timeout(CALL_TIMEOUT, parallel_handle),
        );

        // Always clean up before asserting.
        let _ = client.cancel().await;
        ct.cancel();
        server_handle.await?;

        let result =
            main_result.expect("main call_tool timed out - client may be stuck in endless loop")?;
        let parallel = parallel_result
            .expect("parallel call_tool timed out - client may be stuck in endless loop")?;

        assert_tool_success("parallel", &parallel?);
        assert_tool_success("main", &result);

        Ok(())
    }
}
