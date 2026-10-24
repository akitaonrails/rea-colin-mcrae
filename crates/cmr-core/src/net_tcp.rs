//! A concrete optional transport over TCP, implementing the recovered
//! session lifecycle contract ([`crate::net_transport`]).
//!
//! This is **direct-connect only**: one host and one peer exchange a fixed
//! handshake when the backend is constructed, and the session then runs through
//! the same [`NetworkTransport`] calls the shipped DirectPlay wrappers make.
//! Session discovery, multi-peer fan-out, security, and the game's message
//! synchronization semantics are deliberately out of scope; those are policy
//! and protocol work on top of this contract, not part of faithful recovery.
//!
//! The wire format is a small length-prefixed binary framing owned by this
//! backend (never by the recovered contract): a four-byte magic, a version, and
//! typed frames. Player ids are backend policy: the host's local player is
//! [`HOST_PLAYER`], the joining peer is [`CLIENT_PLAYER`].

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

use crate::net_transport::{
    ApplicationId, IncomingMessage, MessageTarget, NetworkTransport, PlayerId, PlayerInfo,
    PlayerName, ProviderId, ProviderInfo, SessionDesc, SessionFlags, SessionInstanceId,
};

/// Frame magic for the backend's own wire format.
const MAGIC: &[u8; 4] = b"CMRN";
/// Wire format version.
const VERSION: u8 = 1;
/// The provider this backend advertises.
pub const TCP_PROVIDER: ProviderId = ProviderId([
    b'T', b'C', b'P', b'/', b'I', b'P', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]);
/// The host's local player id (backend policy).
pub const HOST_PLAYER: PlayerId = PlayerId(2);
/// The joining peer's player id (backend policy).
pub const CLIENT_PLAYER: PlayerId = PlayerId(4);

const KIND_HELLO: u8 = 1;
const KIND_DESC: u8 = 2;
const KIND_NAME: u8 = 3;
const KIND_MESSAGE: u8 = 4;
const KIND_BYE: u8 = 5;
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Why the TCP backend could not be created or driven.
#[derive(Debug)]
pub enum TcpTransportError {
    Io(io::Error),
    /// The peer's application id does not match the local one.
    ApplicationMismatch,
    /// The wire framing was malformed or used an unsupported version.
    Protocol(&'static str),
    /// The configured host descriptor differs from the lifecycle's descriptor.
    DescriptorMismatch,
    /// The peer closed the connection.
    PeerClosed,
}

impl From<io::Error> for TcpTransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for TcpTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "tcp transport io: {error}"),
            Self::ApplicationMismatch => write!(formatter, "peer application id does not match"),
            Self::Protocol(reason) => write!(formatter, "tcp transport protocol: {reason}"),
            Self::DescriptorMismatch => {
                write!(
                    formatter,
                    "configured session descriptor does not match the lifecycle"
                )
            }
            Self::PeerClosed => write!(formatter, "peer closed the connection"),
        }
    }
}

impl std::error::Error for TcpTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// A direct-connect TCP realization of the recovered session surface.
#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
    descriptor: SessionDesc,
    is_host: bool,
    local_player: Option<PlayerId>,
    players: Vec<PlayerInfo>,
    inbox: VecDeque<IncomingMessage>,
    opened: bool,
    closed: bool,
}

impl TcpTransport {
    /// Hosts the given descriptor: binds were already done by the caller
    /// (`listener`), this accepts the single peer and sends the descriptor.
    pub fn host(listener: TcpListener, descriptor: SessionDesc) -> Result<Self, TcpTransportError> {
        let (mut stream, _) = listener.accept()?;
        stream.set_nodelay(true)?;
        let (kind, hello) = read_frame(&mut stream)?;
        if kind != KIND_HELLO || hello.len() != 16 {
            return Err(TcpTransportError::Protocol("expected a hello frame"));
        }
        write_frame(&mut stream, KIND_DESC, &encode_desc(&descriptor))?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            descriptor,
            is_host: true,
            local_player: None,
            players: Vec::new(),
            inbox: VecDeque::new(),
            opened: false,
            closed: false,
        })
    }

    /// Joins the host at `address`, sending the application id and reading the
    /// advertised descriptor.
    pub fn join(
        address: SocketAddr,
        application: ApplicationId,
    ) -> Result<Self, TcpTransportError> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        write_frame(&mut stream, KIND_HELLO, &application.0)?;
        let (kind, body) = read_frame(&mut stream)?;
        if kind != KIND_DESC {
            return Err(TcpTransportError::Protocol("expected a session descriptor"));
        }
        let descriptor = decode_desc(&body)?;
        if descriptor.application != application {
            return Err(TcpTransportError::ApplicationMismatch);
        }
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            descriptor,
            is_host: false,
            local_player: None,
            players: Vec::new(),
            inbox: VecDeque::new(),
            opened: false,
            closed: false,
        })
    }

    /// The descriptor learned or configured at construction.
    #[must_use]
    pub fn descriptor(&self) -> &SessionDesc {
        &self.descriptor
    }

    /// Whether the session is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.opened
    }

    /// Whether the peer has closed the link or the local teardown ran.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    fn require_open(&self) -> Result<(), TcpTransportError> {
        if !self.opened || self.closed {
            return Err(TcpTransportError::Protocol("session is not open"));
        }
        Ok(())
    }

    fn send_frame(&mut self, kind: u8, body: &[u8]) -> Result<(), TcpTransportError> {
        write_frame(&mut self.stream, kind, body)
    }
}

impl NetworkTransport for TcpTransport {
    type Error = TcpTransportError;

    fn enumerate_providers(&mut self) -> Result<Vec<ProviderInfo>, Self::Error> {
        Ok(vec![ProviderInfo {
            id: TCP_PROVIDER,
            name: "TCP/IP (direct)".to_owned(),
        }])
    }

    fn create(&mut self, provider: ProviderId) -> Result<(), Self::Error> {
        if provider != TCP_PROVIDER {
            return Err(TcpTransportError::Protocol("unsupported provider"));
        }
        Ok(())
    }

    fn set_session_desc(&mut self, desc: &SessionDesc) -> Result<(), Self::Error> {
        // The peer was told the descriptor during the handshake; the lifecycle
        // must use that same session rather than silently redefining it.
        if desc.instance != self.descriptor.instance
            || desc.application != self.descriptor.application
            || desc.name != self.descriptor.name
            || desc.max_players != self.descriptor.max_players
        {
            return Err(TcpTransportError::DescriptorMismatch);
        }
        self.descriptor = desc.clone();
        Ok(())
    }

    fn open(&mut self, desc: &SessionDesc, _join_enabled: bool) -> Result<(), Self::Error> {
        if desc.instance != self.descriptor.instance {
            return Err(TcpTransportError::DescriptorMismatch);
        }
        self.opened = true;
        Ok(())
    }

    fn enumerate_sessions(
        &mut self,
        application: ApplicationId,
    ) -> Result<Vec<SessionDesc>, Self::Error> {
        if application != self.descriptor.application {
            return Ok(Vec::new());
        }
        Ok(vec![self.descriptor.clone()])
    }

    fn create_player(&mut self, name: &PlayerName) -> Result<PlayerId, Self::Error> {
        self.require_open()?;
        if self.local_player.is_some() {
            return Err(TcpTransportError::Protocol("local player already created"));
        }
        let id = if self.is_host {
            HOST_PLAYER
        } else {
            CLIENT_PLAYER
        };
        let mut body = Vec::with_capacity(1 + 4 + 1 + name.short.len() + 1 + name.long.len());
        body.extend_from_slice(&id.0.to_le_bytes());
        push_name(&mut body, &name.short)?;
        push_name(&mut body, &name.long)?;
        self.send_frame(KIND_NAME, &body)?;
        self.local_player = Some(id);
        self.players.push(PlayerInfo {
            id,
            name: name.clone(),
        });
        Ok(id)
    }

    fn enumerate_players(&mut self) -> Result<Vec<PlayerInfo>, Self::Error> {
        self.require_open()?;
        self.poll_frames()?;
        Ok(self.players.clone())
    }

    fn destroy_player(&mut self, player: PlayerId) -> Result<(), Self::Error> {
        if self.local_player == Some(player) {
            self.local_player = None;
        }
        self.players.retain(|info| info.id != player);
        Ok(())
    }

    fn send(
        &mut self,
        from: PlayerId,
        target: MessageTarget,
        payload: &[u8],
    ) -> Result<(), Self::Error> {
        self.require_open()?;
        let mut body = Vec::with_capacity(9 + payload.len());
        body.extend_from_slice(&from.0.to_le_bytes());
        let (kind, id) = match target {
            MessageTarget::Player(id) => (0u8, id.0),
            MessageTarget::AllPlayers => (1, 0),
            MessageTarget::Server => (2, 0),
        };
        body.push(kind);
        body.extend_from_slice(&id.to_le_bytes());
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        self.send_frame(KIND_MESSAGE, &body)
    }

    fn receive(&mut self) -> Result<Option<IncomingMessage>, Self::Error> {
        self.require_open()?;
        self.poll_frames()?;
        Ok(self.inbox.pop_front())
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        if self.closed {
            return Ok(());
        }
        let _ = self.send_frame(KIND_BYE, &[]);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        self.closed = true;
        self.opened = false;
        self.players.clear();
        self.local_player = None;
        Ok(())
    }
}

impl TcpTransport {
    fn poll_frames(&mut self) -> Result<(), TcpTransportError> {
        loop {
            match read_frame_nonblocking(&mut self.stream) {
                Ok(Some((kind, body))) => self.handle_frame(kind, body)?,
                Ok(None) | Err(TcpTransportError::PeerClosed) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    fn handle_frame(&mut self, kind: u8, body: Vec<u8>) -> Result<(), TcpTransportError> {
        match kind {
            KIND_NAME => {
                let mut cursor = Cursor::new(&body);
                let id = PlayerId(cursor.u32()?);
                let short = cursor.name()?;
                let long = cursor.name()?;
                if !self.players.iter().any(|info| info.id == id) {
                    self.players.push(PlayerInfo {
                        id,
                        name: PlayerName { short, long },
                    });
                }
                Ok(())
            }
            KIND_MESSAGE => {
                let mut cursor = Cursor::new(&body);
                let from = PlayerId(cursor.u32()?);
                let target_kind = cursor.u8()?;
                let target_id = cursor.u32()?;
                let length = cursor.u32()? as usize;
                let payload = cursor.bytes(length)?.to_vec();
                let to = match target_kind {
                    0 => PlayerId(target_id),
                    1 => PlayerId::ALL,
                    2 => PlayerId::SERVER,
                    _ => return Err(TcpTransportError::Protocol("unknown message target")),
                };
                self.inbox.push_back(IncomingMessage { from, to, payload });
                Ok(())
            }
            KIND_BYE => {
                self.closed = true;
                self.opened = false;
                Ok(())
            }
            _ => Err(TcpTransportError::Protocol("unknown frame kind")),
        }
    }
}

// `inbox` is declared below with the struct's other fields.

fn encode_desc(desc: &SessionDesc) -> Vec<u8> {
    let mut body = Vec::with_capacity(16 + 16 + 16 + 4 + 4 + 4 + 2 + desc.name.len());
    body.extend_from_slice(&desc.instance.0);
    body.extend_from_slice(&desc.application.0);
    body.extend_from_slice(&desc.max_players.to_le_bytes());
    body.extend_from_slice(&desc.current_players.to_le_bytes());
    body.extend_from_slice(&desc.flags.bits().to_le_bytes());
    body.extend_from_slice(&(desc.name.len() as u16).to_le_bytes());
    body.extend_from_slice(desc.name.as_bytes());
    body
}

fn decode_desc(body: &[u8]) -> Result<SessionDesc, TcpTransportError> {
    let mut cursor = Cursor::new(body);
    let instance = SessionInstanceId(cursor.array16()?);
    let application = ApplicationId(cursor.array16()?);
    let max_players = cursor.u32()?;
    let current_players = cursor.u32()?;
    let bits = cursor.u32()?;
    let name_length = cursor.u16()? as usize;
    let name = String::from_utf8(cursor.bytes(name_length)?.to_vec())
        .map_err(|_| TcpTransportError::Protocol("session name is not UTF-8"))?;
    if cursor.remaining() != 0 {
        return Err(TcpTransportError::Protocol("trailing descriptor bytes"));
    }
    Ok(SessionDesc {
        instance,
        application,
        max_players,
        current_players,
        name,
        flags: SessionFlags {
            migrate_host: bits & 0x04 != 0,
            keep_alive: bits & 0x40 != 0,
            join_disabled: bits & 0x20 != 0,
        },
    })
}

fn push_name(body: &mut Vec<u8>, value: &str) -> Result<(), TcpTransportError> {
    let length = u8::try_from(value.len())
        .map_err(|_| TcpTransportError::Protocol("player name is too long for the frame"))?;
    body.push(length);
    body.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], TcpTransportError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(TcpTransportError::Protocol("frame length overflow"))?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(TcpTransportError::Protocol("truncated frame"))?;
        self.offset = end;
        Ok(slice)
    }

    fn array16(&mut self) -> Result<[u8; 16], TcpTransportError> {
        self.bytes(16)?
            .try_into()
            .map_err(|_| TcpTransportError::Protocol("truncated id"))
    }

    fn u8(&mut self) -> Result<u8, TcpTransportError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, TcpTransportError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().map_err(
            |_| TcpTransportError::Protocol("truncated u16"),
        )?))
    }

    fn u32(&mut self) -> Result<u32, TcpTransportError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().map_err(
            |_| TcpTransportError::Protocol("truncated u32"),
        )?))
    }

    fn name(&mut self) -> Result<String, TcpTransportError> {
        let length = self.u8()? as usize;
        String::from_utf8(self.bytes(length)?.to_vec())
            .map_err(|_| TcpTransportError::Protocol("player name is not UTF-8"))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
}

fn write_frame(stream: &mut TcpStream, kind: u8, body: &[u8]) -> Result<(), TcpTransportError> {
    if body.len() > MAX_FRAME_BYTES {
        return Err(TcpTransportError::Protocol("frame too large"));
    }
    let mut frame = Vec::with_capacity(10 + body.len());
    frame.extend_from_slice(MAGIC);
    frame.push(VERSION);
    frame.push(kind);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body);
    let mut written = 0;
    let mut retries = 0;
    while written < frame.len() {
        match stream.write(&frame[written..]) {
            Ok(0) => return Err(TcpTransportError::PeerClosed),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                retries += 1;
                if retries > 10_000 {
                    return Err(TcpTransportError::Io(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "peer is not draining the socket",
                    )));
                }
                std::thread::yield_now();
            }
            Err(error) => return Err(TcpTransportError::Io(error)),
        }
    }
    Ok(())
}

fn read_frame(stream: &mut TcpStream) -> Result<(u8, Vec<u8>), TcpTransportError> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut header = [0u8; 10];
    stream.read_exact(&mut header)?;
    if &header[0..4] != MAGIC || header[4] != VERSION {
        return Err(TcpTransportError::Protocol("bad frame header"));
    }
    let length = u32::from_le_bytes([header[6], header[7], header[8], header[9]]) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(TcpTransportError::Protocol("frame too large"));
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok((header[5], body))
}

fn read_frame_nonblocking(
    stream: &mut TcpStream,
) -> Result<Option<(u8, Vec<u8>)>, TcpTransportError> {
    let mut header = [0u8; 10];
    let mut filled = 0;
    while filled < header.len() {
        match stream.read(&mut header[filled..]) {
            Ok(0) => return Err(TcpTransportError::PeerClosed),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if filled == 0 {
                    return Ok(None);
                }
                return Err(TcpTransportError::Protocol("partial frame header"));
            }
            Err(error) => return Err(TcpTransportError::Io(error)),
        }
    }
    if &header[0..4] != MAGIC || header[4] != VERSION {
        return Err(TcpTransportError::Protocol("bad frame header"));
    }
    let length = u32::from_le_bytes([header[6], header[7], header[8], header[9]]) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(TcpTransportError::Protocol("frame too large"));
    }
    let mut body = vec![0u8; length];
    let mut filled = 0;
    while filled < length {
        match stream.read(&mut body[filled..]) {
            Ok(0) => return Err(TcpTransportError::PeerClosed),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(TcpTransportError::Protocol("partial frame body"));
            }
            Err(error) => return Err(TcpTransportError::Io(error)),
        }
    }
    Ok(Some((header[5], body)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_transport::{NetworkError, SessionLifecycle, TransportState};
    use std::time::{Duration, Instant};

    fn application() -> ApplicationId {
        ApplicationId([0x11; 16])
    }

    fn instance() -> SessionInstanceId {
        SessionInstanceId([0x22; 16])
    }

    fn descriptor() -> SessionDesc {
        SessionDesc::hosting(instance(), application(), "TCP Rally", 4).unwrap()
    }

    fn player(short: &str) -> PlayerName {
        PlayerName::new(short, short).unwrap()
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for the peer");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn tcp_host_and_join_run_the_recovered_lifecycle_and_exchange_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let host = std::thread::spawn(move || {
            let mut transport = TcpTransport::host(listener, descriptor()).unwrap();
            let mut lifecycle = SessionLifecycle::new();
            lifecycle.start(&mut transport, TCP_PROVIDER).unwrap();
            lifecycle
                .host(&mut transport, descriptor(), &player("Host"))
                .unwrap();
            wait_until(|| transport.enumerate_players().unwrap().len() == 2);
            assert_eq!(lifecycle.local_player(), Some(HOST_PLAYER));
            lifecycle
                .send(&mut transport, MessageTarget::AllPlayers, b"hello")
                .unwrap();
            let message = wait_until_receive(&mut lifecycle, &mut transport);
            assert_eq!(message.from, CLIENT_PLAYER);
            assert_eq!(message.payload, b"sync");
            lifecycle.close(&mut transport).unwrap();
            lifecycle.close(&mut transport).unwrap();
            assert_eq!(lifecycle.state(), &TransportState::Idle);
            assert!(transport.is_closed());
        });

        let mut transport = TcpTransport::join(address, application()).unwrap();
        assert_eq!(transport.descriptor(), &descriptor());
        let mut lifecycle = SessionLifecycle::new();
        lifecycle.start(&mut transport, TCP_PROVIDER).unwrap();
        lifecycle
            .join(&mut transport, application(), instance(), &player("Client"))
            .unwrap();
        assert_eq!(lifecycle.local_player(), Some(CLIENT_PLAYER));
        wait_until(|| transport.enumerate_players().unwrap().len() == 2);
        let message = wait_until_receive(&mut lifecycle, &mut transport);
        assert_eq!(message.from, HOST_PLAYER);
        assert_eq!(message.payload, b"hello");
        lifecycle
            .send(&mut transport, MessageTarget::Server, b"sync")
            .unwrap();
        lifecycle.close(&mut transport).unwrap();
        host.join().unwrap();
    }

    fn wait_until_receive(
        lifecycle: &mut SessionLifecycle,
        transport: &mut TcpTransport,
    ) -> IncomingMessage {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(message) = lifecycle.receive(transport).unwrap() {
                return message;
            }
            assert!(Instant::now() < deadline, "timed out waiting for a message");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn joining_a_mismatched_application_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let _ = TcpTransport::host(listener, descriptor());
        });
        let error = TcpTransport::join(address, ApplicationId([0x33; 16])).unwrap_err();
        assert!(matches!(error, TcpTransportError::ApplicationMismatch));
        let _ = host.join();
    }

    #[test]
    fn a_descriptor_that_differs_from_the_handshake_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let mut transport = TcpTransport::host(listener, descriptor()).unwrap();
            let mut lifecycle = SessionLifecycle::new();
            lifecycle.start(&mut transport, TCP_PROVIDER).unwrap();
            let other =
                SessionDesc::hosting(SessionInstanceId([0x44; 16]), application(), "Other", 2)
                    .unwrap();
            let error = lifecycle
                .host(&mut transport, other, &player("Host"))
                .unwrap_err();
            assert!(matches!(
                error,
                NetworkError::Backend {
                    source: TcpTransportError::DescriptorMismatch,
                    ..
                }
            ));
            lifecycle.close(&mut transport).unwrap();
        });
        let _ = TcpTransport::join(address, application());
        host.join().unwrap();
    }
}
