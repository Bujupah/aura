use std::time::Duration;

use base64::Engine;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::event::{ClientEvent, ServerEvent, SessionConfig};
use crate::{ApiCredential, LiveError};

const ENDPOINT: &str = "wss://api.openai.com/v1/live/sessions";
const START_TIMEOUT: Duration = Duration::from_secs(15);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// An established session: `session.started` has been received.
pub struct LiveConnection {
    pub session_id: String,
    socket: Socket,
}

/// Opens a session and waits until the server is ready for audio.
pub async fn connect(
    credential: &ApiCredential,
    config: SessionConfig,
) -> Result<LiveConnection, LiveError> {
    let mut request = ENDPOINT
        .into_client_request()
        .map_err(|error| LiveError::Request(error.to_string()))?;
    let mut authorization = HeaderValue::from_str(&credential.bearer())
        .map_err(|_| LiveError::Request("the API token contains invalid characters".into()))?;
    authorization.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, authorization);

    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    send(&mut socket, &ClientEvent::SessionStart { session: config }).await?;

    let started = tokio::time::timeout(START_TIMEOUT, async {
        loop {
            match next_event(&mut socket).await {
                Some(Ok(ServerEvent::SessionStarted { session_id })) => return Ok(session_id),
                Some(Ok(ServerEvent::Error(error))) => return Err(LiveError::Rejected(error)),
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Err(error),
                None => return Err(LiveError::ClosedBefore("session.started")),
            }
        }
    })
    .await
    .map_err(|_| LiveError::Timeout {
        event: "session.started",
        seconds: START_TIMEOUT.as_secs(),
    })??;

    Ok(LiveConnection {
        session_id: started,
        socket,
    })
}

impl LiveConnection {
    /// Audio is sent and events are received concurrently for the whole
    /// session, so the two halves go to separate tasks.
    pub fn split(self) -> (LiveSender, LiveReceiver) {
        let (sink, stream) = self.socket.split();
        (LiveSender { sink }, LiveReceiver { stream })
    }
}

pub struct LiveSender {
    sink: SplitSink<Socket, Message>,
}

impl LiveSender {
    /// Appends PCM16 audio. The stream must be continuous and ordered —
    /// silence included — at the session's sample rate.
    pub async fn append_audio(&mut self, pcm: &[u8]) -> Result<(), LiveError> {
        if !pcm.len().is_multiple_of(2) {
            return Err(LiveError::PartialSample(pcm.len()));
        }
        let audio = base64::engine::general_purpose::STANDARD.encode(pcm);
        self.send(&ClientEvent::InputAudioAppend { audio }).await
    }

    pub async fn send(&mut self, event: &ClientEvent) -> Result<(), LiveError> {
        send(&mut self.sink, event).await
    }

    /// Asks the server to finalize. Keep reading until
    /// [`ServerEvent::SessionClosed`]; it carries the final usage.
    pub async fn close(&mut self) -> Result<(), LiveError> {
        self.send(&ClientEvent::SessionClose).await
    }
}

pub struct LiveReceiver {
    stream: SplitStream<Socket>,
}

impl LiveReceiver {
    /// `None` once the connection has closed.
    pub async fn next_event(&mut self) -> Option<Result<ServerEvent, LiveError>> {
        next_event(&mut self.stream).await
    }
}

async fn send<S>(sink: &mut S, event: &ClientEvent) -> Result<(), LiveError>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(event)?;
    sink.send(Message::Text(text.into())).await?;
    Ok(())
}

async fn next_event<S>(stream: &mut S) -> Option<Result<ServerEvent, LiveError>>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        return match stream.next().await? {
            Ok(Message::Text(text)) => Some(ServerEvent::parse(&text).map_err(LiveError::from)),
            Ok(Message::Close(_)) => None,
            // Pings are answered by the transport; nothing else carries events.
            Ok(_) => continue,
            Err(error) => Some(Err(error.into())),
        };
    }
}
