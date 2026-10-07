//! Runs a listening session from the terminal and prints its events as JSON
//! lines. Stop with Ctrl-C, or automatically after `--seconds N`.
//!
//!   cargo run -p aura-session --example listen -- --fixtures seller.pcm customer.pcm --seconds 25
//!   cargo run -p aura-session --example listen            # this Mac's microphone and system audio
//!
//! Add `--no-web` to keep the note-taking agent off the internet.
//! Both sessions are billed per second while this runs.

use std::time::Duration;

use aura_live::ApiCredential;
use aura_session::{
    AudioInput, Credentials, Incoming, ListeningState, MeetingSession, SessionEvent, SessionOptions,
    Translation,
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut audio = AudioInput::Capture;
    let mut seconds = None;
    let mut options = SessionOptions { web_access: true, illustrations: true, translation: None };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--fixtures" => {
                let seller = args.get(index + 1).ok_or("--fixtures needs two files")?;
                let customer = args.get(index + 2).ok_or("--fixtures needs two files")?;
                audio = AudioInput::Fixtures { seller: seller.into(), customer: customer.into() };
                index += 3;
            }
            "--seconds" => {
                seconds = Some(args.get(index + 1).ok_or("--seconds needs a number")?.parse::<u64>()?);
                index += 2;
            }
            // --translate <my language> <meeting language>: show the meeting
            // translated into my language (text only, nothing is played).
            "--translate" => {
                let mine = args.get(index + 1).ok_or("--translate needs two language codes")?;
                let meeting = args.get(index + 2).ok_or("--translate needs two language codes")?;
                options.translation = Some(Translation {
                    my_language: mine.clone(),
                    meeting_language: meeting.clone(),
                    incoming: Incoming::Text,
                    outgoing_device_uid: None,
                });
                index += 3;
            }
            // --translate-voice <my language> <meeting language> <device>:
            // interpret my speech into the meeting's language and play it into
            // the output device whose name contains <device>.
            "--translate-voice" => {
                let mine = args.get(index + 1).ok_or("--translate-voice needs two languages and a device")?;
                let meeting = args.get(index + 2).ok_or("--translate-voice needs two languages and a device")?;
                let device = args.get(index + 3).ok_or("--translate-voice needs two languages and a device")?;
                let uid = aura_audio::playback::find_output_device(device)
                    .ok_or_else(|| format!("no audio output device named like {device}"))?;
                options.translation = Some(Translation {
                    my_language: mine.clone(),
                    meeting_language: meeting.clone(),
                    incoming: Incoming::Off,
                    outgoing_device_uid: Some(uid),
                });
                index += 4;
            }
            "--no-images" => {
                options.illustrations = false;
                index += 1;
            }
            "--no-web" => {
                options.web_access = false;
                index += 1;
            }
            other => return Err(format!("unknown argument {other}").into()),
        }
    }

    let credentials = Credentials {
        openai: ApiCredential::from_env()?,
        gemini: ApiCredential::from_env_var(aura_translate::API_TOKEN_ENV).ok(),
    };
    let (events, mut received) = mpsc::unbounded_channel();
    let session = MeetingSession::start(&credentials, audio, options, events).await?;

    let limit = async {
        match seconds {
            Some(seconds) => tokio::time::sleep(Duration::from_secs(seconds)).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(limit);
    let mut level_ticks = 0u32;
    loop {
        tokio::select! {
            Some(event) = received.recv() => match &event {
                // Ten level events a second would bury the transcript.
                SessionEvent::Levels { .. } => {
                    level_ticks += 1;
                    if level_ticks.is_multiple_of(20) { println!("{}", serde_json::to_string(&event)?); }
                }
                SessionEvent::Turn { turn } if !turn.is_final => {}
                SessionEvent::TopicImage { topic_id, png } => {
                    println!("{{\"type\":\"topicImage\",\"topicId\":\"{topic_id}\",\"bytes\":{}}}", png.len());
                }
                SessionEvent::State { state: ListeningState::Failed { .. } } => {
                    println!("{}", serde_json::to_string(&event)?);
                    break;
                }
                _ => println!("{}", serde_json::to_string(&event)?),
            },
            _ = &mut limit => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    let stopping = tokio::spawn(session.stop());
    // Turns finalized during shutdown still arrive here.
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(20), received.recv()).await {
        if matches!(&event, SessionEvent::Turn { turn } if turn.is_final)
            || matches!(&event, SessionEvent::Topics { .. } | SessionEvent::Advice { .. })
        {
            println!("{}", serde_json::to_string(&event)?);
        }
    }
    let ended = stopping.await?;
    println!(
        "{{\"type\":\"usage\",\"seconds\":{},\"confirmed\":{},\"turns\":{}}}",
        ended.usage.seconds,
        ended.usage.confirmed,
        ended.transcript.len()
    );
    Ok(())
}
