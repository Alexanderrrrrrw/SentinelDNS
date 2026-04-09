use std::sync::Arc;
use std::time::Instant;

use sentinel_policy::SentinelPolicyEngine;
use sentinel_resolver::SentinelResolver;
use sentinel_types::{BlockMode, PolicyEngine};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let policy: Arc<dyn PolicyEngine> =
        Arc::new(SentinelPolicyEngine::from_blocklist_text(BlockMode::Nxdomain, ""));
    let resolver = SentinelResolver::new(policy, BlockMode::Nxdomain);

    let iterations = 10;
    let start = Instant::now();
    let mut total_ms = 0u64;
    for _ in 0..iterations {
        let result = resolver.resolve_domain("example.com").await?;
        total_ms += result.response_time_ms;
    }
    let wall_ms = start.elapsed().as_millis();
    println!("iterations={iterations} total_response_ms={total_ms} wall_ms={wall_ms}");
    println!(
        "avg_response_ms={}",
        sentinel_resolver::synthetic_chain_benchmark_avg(total_ms, iterations)
    );
    Ok(())
}
