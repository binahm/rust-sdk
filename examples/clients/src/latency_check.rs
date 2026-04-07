use std::time::Instant;

use clap::Parser;
use rmcp::{
    ServiceExt,
    model::*,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

#[derive(Parser)]
struct Args {
    /// MCP server URL
    #[arg(long, default_value = "http://localhost:8000/mcp")]
    url: String,

    /// Tool name to call
    #[arg(long, default_value = "say_hello")]
    tool: String,

    /// Use a custom reqwest client with no connection pooling
    #[arg(long)]
    custom_client: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let url = args.url.clone();
    let tool = args.tool.clone();
    let use_custom_client = args.custom_client;

    let client = if use_custom_client {
        println!("Using custom reqwest client with no connection pooling");
        let reqwest_client = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()?;

        let transport = StreamableHttpClientTransport::with_client(
            reqwest_client,
            StreamableHttpClientTransportConfig::with_uri(url),
        );
        ClientInfo::default().serve(transport).await?
    } else {
        let transport = StreamableHttpClientTransport::from_uri(url);
        ClientInfo::default().serve(transport).await?
    };

    if tool == "ping" {
        println!("Starting loop for sending ping requests to {}", args.url);
    } else {
        println!("Staring loop for calling tool '{}' at: {}", tool, args.url);
    }

    for i in 1..=5 {
        let start = Instant::now();
        if tool == "ping" {
            client
                .send_request(ClientRequest::PingRequest(PingRequest::default()))
                .await?;
        } else {
            client
                .call_tool(CallToolRequestParams::new(tool.clone()))
                .await?;
        }
        println!(
            "Call {}: {:.1}ms",
            i,
            start.elapsed().as_secs_f32() * 1000.0
        );
    }

    client.cancel().await?;
    Ok(())
}
