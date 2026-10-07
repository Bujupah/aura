//! Asks the illustrator sub-agent for one picture and writes it to a file.
//! A development probe; each run is one billed image.
//!
//!   cargo run -p aura-intel --example illustrate -- out.png "AWS and OpenShift feeding a CMDB"

use aura_core::topics::Topic;
use aura_intel::{Illustrator, ResponsesClient};
use aura_live::ApiCredential;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: illustrate <out.png> <brief>")?;
    let brief = args.next().ok_or("usage: illustrate <out.png> <brief>")?;

    let client = ResponsesClient::new(ApiCredential::from_env()?, "gpt-5.6-luna")?;
    let topic = Topic {
        id: "probe".into(),
        title: "Current environment".into(),
        notes: vec!["Customer: runs on AWS and OpenShift.".into(), "Customer: CMDB gets outdated quickly.".into()],
        source_turn_ids: Vec::new(),
        sources: Vec::new(),
        diagram: None,
        image: None,
        updated_at_ms: 0,
        placement: None,
    };
    let started = std::time::Instant::now();
    let png = Illustrator::new(client).illustrate(&brief, &topic).await?;
    std::fs::write(&path, &png)?;
    println!("{} bytes in {:.1}s -> {path}", png.len(), started.elapsed().as_secs_f32());
    Ok(())
}
