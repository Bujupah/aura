//! Streams a raw PCM16 / 16 kHz mono file to Gemini Live Translate at
//! real-time pace, prints what it heard and said, and writes the translated
//! speech (PCM16 / 24 kHz mono) next to the input.
//!
//!   cargo run -p aura-translate --example translate -- clip16k.pcm es

use std::time::{Duration, Instant};

use aura_live::ApiCredential;
use aura_translate::{connect, TranslateConfig, TranslateEvent, API_TOKEN_ENV, INPUT_SAMPLE_RATE};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: translate <file16k.pcm> <language>")?;
    let target = args.next().ok_or("usage: translate <file16k.pcm> <language>")?;

    let mut audio = tokio::fs::read(&path).await?;
    audio.truncate(audio.len() - audio.len() % 2);
    let chunk = INPUT_SAMPLE_RATE as usize * 2 / 10;
    audio.resize(audio.len() + chunk * 80, 0); // 8 s of silence for the tail

    let credential = ApiCredential::from_env_var(API_TOKEN_ENV)?;
    let config = TranslateConfig { target_language: target.clone(), echo_target_language: false };
    let (mut sender, mut receiver) = connect(&credential, &config).await?.split();
    let started = Instant::now();

    let sending = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        for chunk in audio.chunks(chunk) {
            ticker.tick().await;
            if sender.append_audio(chunk).await.is_err() {
                break;
            }
        }
        let _ = sender.close().await;
    });

    let (mut heard, mut said, mut speech, mut first_audio) = (String::new(), String::new(), Vec::new(), None);
    while let Some(events) = receiver.next_events().await {
        for event in events? {
            match event {
                TranslateEvent::Heard { text, .. } => heard.push_str(&text),
                TranslateEvent::Said { text } => said.push_str(&text),
                TranslateEvent::Audio(audio) => {
                    first_audio.get_or_insert(started.elapsed());
                    speech.extend(audio);
                }
            }
        }
    }
    sending.await?;

    let out = format!("{path}.{target}.pcm");
    tokio::fs::write(&out, &speech).await?;
    println!("heard: {}", heard.trim());
    println!("said:  {}", said.trim());
    println!(
        "translated speech: {:.1}s, first audio after {:.1}s -> {out}",
        speech.len() as f32 / 48_000.0,
        first_audio.map_or(f32::NAN, |at: Duration| at.as_secs_f32())
    );
    Ok(())
}
