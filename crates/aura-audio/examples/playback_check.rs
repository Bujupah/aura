//! Opens the default audio output and plays one second of a tone at zero
//! volume: a silent check that the playback path works on this Mac. Also
//! reports whether a virtual microphone device is installed.
//!
//!   cargo run -p aura-audio --example playback_check

#[cfg(target_os = "macos")]
fn main() {
    use aura_audio::playback::{default_output_device, find_output_device, Playback};

    let virtual_microphone = find_output_device("BlackHole");
    match &virtual_microphone {
        Some(uid) => println!("virtual microphone: found ({uid})"),
        None => println!("virtual microphone: none installed"),
    }
    match (default_output_device(), &virtual_microphone) {
        (Some(output), Some(uid)) if output == *uid => println!(
            "Mac sound output: IS the virtual microphone — you will hear nothing; choose your headphones or speakers"
        ),
        (Some(_), _) => println!("Mac sound output: a real device (good)"),
        (None, _) => println!("Mac sound output: unknown"),
    }
    match Playback::open(None, 24_000, 0.0) {
        Ok(playback) => {
            let tone: Vec<i16> = (0..24_000)
                .map(|n| ((n as f32 * 440.0 * std::f32::consts::TAU / 24_000.0).sin() * 8_000.0) as i16)
                .collect();
            playback.write(&tone);
            std::thread::sleep(std::time::Duration::from_millis(1200));
            println!("default output: opened and played 1 s silently");
        }
        Err(error) => println!("default output: {error}"),
    }
    match Playback::open(Some("no-such-device"), 24_000, 0.0) {
        Ok(_) => println!("unknown device: unexpectedly opened"),
        Err(_) => println!("unknown device: refused, as it should be"),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
