mod commands;
mod languages;
mod meeting;
mod settings;
mod shell;
mod shortcuts;
mod tokens;
mod topics;
mod tray;
mod windows;

/// Structured logs go to stderr and to `~/Library/Logs/Aura/aura.log`, so a
/// session can be diagnosed after the fact however the app was launched.
/// They carry event names, states and counts — never transcript text.
fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

    let file = std::env::var_os("HOME").and_then(|home| {
        let directory = std::path::PathBuf::from(home).join("Library/Logs/Aura");
        std::fs::create_dir_all(&directory).ok()?;
        let path = directory.join("aura.log");
        // Start afresh once the file has grown large.
        let oversized = std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_LOG_BYTES);
        std::fs::OpenOptions::new()
            .create(true)
            .append(!oversized)
            .write(true)
            .truncate(oversized)
            .open(path)
            .ok()
    });
    tracing_subscriber::registry()
        // Without this, window-system libraries flood the log with traces.
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(tracing_subscriber::fmt::layer().json().with_writer(std::io::stderr))
        .with(file.map(|file| tracing_subscriber::fmt::layer().json().with_writer(std::sync::Mutex::new(file))))
        .init();
}

pub fn run() {
    init_logging();

    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .plugin(shortcuts::plugin())
        .manage(shell::Shell::default())
        .manage(meeting::Meeting::default())
        .manage(topics::TopicWindows::default())
        .manage(settings::SettingsStore::default())
        .invoke_handler(tauri::generate_handler![
            commands::shell_state,
            commands::shell_shortcuts,
            commands::shell_dispatch,
            commands::app_quit,
            meeting::meeting_state,
            meeting::meeting_start,
            meeting::meeting_stop,
            topics::topics_current,
            topics::topic_dismiss,
            topics::topic_image,
            settings::settings_get,
            tokens::tokens_status,
            tokens::token_set,
            tokens::token_clear,
            tokens::tokens_open,
        ])
        .setup(|app| {
            // Menu-bar app: no Dock icon, and showing a panel never pulls the
            // seller out of their meeting app.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            settings::load(handle);
            windows::init(handle)?;
            tray::init(handle)?;
            shortcuts::register(handle);
            shell::present(handle);
            tracing::info!(event = "shell_started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Aura");
}
