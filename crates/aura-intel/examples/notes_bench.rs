//! Times the note-taking agent on a scripted conversation, one turn at a
//! time, without any audio. Each turn is one billed model call.
//!
//!   cargo run -p aura-intel --example notes_bench            # web search available
//!   cargo run -p aura-intel --example notes_bench -- --no-web

use std::time::{Duration, Instant};

use aura_core::transcript::{Speaker, Turn};
use aura_intel::{track_topics, Abilities, Earlier, ResponsesClient, Update};
use aura_live::ApiCredential;
use tokio::sync::mpsc;

const SCRIPT: &[(Speaker, &str)] = &[
    (Speaker::Customer, "Honestly, our CMDB gets outdated very quickly, nobody trusts it."),
    (Speaker::Seller, "How are you discovering your infrastructure today?"),
    (Speaker::Customer, "Mostly spreadsheets, and a script someone wrote years ago. We run on AWS and OpenShift."),
    (Speaker::Customer, "Dynatrace and Splunk both send alerts into ServiceNow, which creates an incident. Finding the root cause takes hours."),
    (Speaker::Customer, "Does BMC Helix Discovery support OpenShift?"),
    (Speaker::Seller, "Let me confirm the exact supported versions and send you a reference architecture by Friday."),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let web_search = !std::env::args().any(|argument| argument == "--no-web");
    let client = ResponsesClient::new(ApiCredential::from_env()?, "gpt-5.6-luna")?;
    let (turns, received) = mpsc::unbounded_channel();
    let (updates, mut updated) = mpsc::unbounded_channel();
    let abilities = Abilities { web_search, illustrator: None };
    let (_control, controls) = mpsc::unbounded_channel();
    tokio::spawn(track_topics(client, abilities, Earlier::default(), received, controls, move |update| {
        let _ = updates.send(update);
    }));

    let mut latencies = Vec::new();
    for (index, (speaker, text)) in SCRIPT.iter().enumerate() {
        let prefix = if *speaker == Speaker::Seller { "seller" } else { "customer" };
        turns.send(Turn {
            id: format!("{prefix}-{index}"),
            speaker: *speaker,
            text: (*text).to_owned(),
            start_ms: index as u64 * 10_000,
            end_ms: index as u64 * 10_000 + 5_000,
            is_final: true,
            translation: None,
        })?;
        let sent = Instant::now();
        // A turn that changes nothing produces no update; give it a bound.
        match tokio::time::timeout(Duration::from_secs(45), updated.recv()).await {
            Ok(Some(Update::Topics(topics))) => {
                let seconds = sent.elapsed().as_secs_f32();
                latencies.push(seconds);
                let shown = topics.iter().filter(|topic| topic.placement.is_some()).count();
                let diagrams = topics.iter().filter(|topic| topic.diagram.is_some()).count();
                let web = topics.iter().filter(|topic| !topic.sources.is_empty()).count();
                println!("turn {index}: {seconds:>5.1}s  windows={shown} diagrams={diagrams} with_web_sources={web}");
            }
            Ok(_) => println!("turn {index}: other update"),
            Err(_) => println!("turn {index}: no change within 45s"),
        }
    }
    latencies.sort_by(f32::total_cmp);
    if let (Some(fastest), Some(slowest)) = (latencies.first(), latencies.last()) {
        let mean = latencies.iter().sum::<f32>() / latencies.len() as f32;
        println!("web_search={web_search}  fastest {fastest:.1}s  mean {mean:.1}s  slowest {slowest:.1}s");
    }
    Ok(())
}
