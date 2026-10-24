//! Optional portable network transport reconstructed from the shipped
//! DirectPlay session surface (H09/H10).
//!
//! The shipped game creates multiplayer sessions through `DPLAYX.dll`:
//! `DirectPlayEnumerateA` fills a service-provider table, `DirectPlayCreate` +
//! `QueryInterface(IID_IDirectPlay3A)` builds the session object, and the
//! recovered wrappers then host or join one session, create/enumerate/destroy
//! players, and `Send`/`Receive` size-probed messages until teardown. This
//! module restates that lifecycle as a backend-neutral contract so an optional
//! modern transport (or an in-memory fake in tests) can implement the same
//! sequence without DirectPlay.
//!
//! It is deliberately *only* the game-facing contract: provider protocols,
//! GUID negotiation, and packet framing live inside the original DLL and are
//! not reconstructed here. The recovered session description and flags are
//! carried verbatim, and the lifecycle state machine fails closed on any
//! ordering the shipped code never performs.

use std::fmt;

/// The shipped build's per-player network table has 12 slots (`0x9c` stride,
/// eleven `+0x218` records plus the local entry), so a recovered session cannot
/// exceed twelve players.
pub const MAX_SESSION_PLAYERS: u32 = 12;

/// The provider-identifying GUID the original passes to `DirectPlayCreate`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderId(pub [u8; 16]);

/// The application GUID a session is advertised under (`guidApplication`).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ApplicationId(pub [u8; 16]);

/// One session's `guidInstance`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionInstanceId(pub [u8; 16]);

/// A player or group id. `0` and `1` are the DirectPlay system/server
/// constants, so a real player id is never either of them.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PlayerId(pub u32);

impl PlayerId {
    /// `DPID_ALLPLAYERS` from the recovered header.
    pub const ALL: Self = Self(0);
    /// `DPID_SERVERPLAYER` from the recovered header.
    pub const SERVER: Self = Self(1);
}

/// A player's `DPNAME` pair. The original copies a 25-byte long name and a
/// 15-byte short name from the enumeration callback, so both are bounded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlayerName {
    pub short: String,
    pub long: String,
}

impl PlayerName {
    /// Longest long name the recovered callback copies (25 bytes).
    pub const MAX_LONG: usize = 24;
    /// Longest short name the recovered callback copies (15 bytes).
    pub const MAX_SHORT: usize = 14;

    /// Builds a name, truncating at the recovered byte limits on character
    /// boundaries (the original uses `strncpy`), or rejecting empty names.
    pub fn new(short: &str, long: &str) -> Result<Self, TransportError> {
        if short.is_empty() && long.is_empty() {
            return Err(TransportError::EmptyPlayerName);
        }
        Ok(Self {
            short: truncate(short, Self::MAX_SHORT),
            long: truncate(long, Self::MAX_LONG),
        })
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// The recovered `DPSESSION_*` bits the shipped game uses.
///
/// The host path sets `MIGRATEHOST | KEEPALIVE` (`0x44`) and toggles
/// `JOINDISABLED` (`0x20`) from a caller argument when it opens the session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionFlags {
    pub migrate_host: bool,
    pub keep_alive: bool,
    pub join_disabled: bool,
}

impl SessionFlags {
    /// The exact host flags observed in the shipped descriptor (`0x44`).
    pub const HOST_DEFAULT: Self = Self {
        migrate_host: true,
        keep_alive: true,
        join_disabled: false,
    };

    /// The recovered numeric word (`0x44` before the join gate is applied).
    #[must_use]
    pub const fn bits(self) -> u32 {
        (if self.migrate_host { 0x04 } else { 0 })
            | (if self.keep_alive { 0x40 } else { 0 })
            | (if self.join_disabled { 0x20 } else { 0 })
    }
}

/// The recovered session descriptor (`DPSESSIONDESC2`, `0x50` bytes).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDesc {
    pub instance: SessionInstanceId,
    pub application: ApplicationId,
    pub max_players: u32,
    pub current_players: u32,
    pub name: String,
    pub flags: SessionFlags,
}

impl SessionDesc {
    /// Builds a host descriptor with the recovered flags and validates the
    /// player bounds against the 12-slot network table.
    pub fn hosting(
        instance: SessionInstanceId,
        application: ApplicationId,
        name: &str,
        max_players: u32,
    ) -> Result<Self, TransportError> {
        if name.is_empty() {
            return Err(TransportError::EmptySessionName);
        }
        if max_players == 0 || max_players > MAX_SESSION_PLAYERS {
            return Err(TransportError::InvalidMaxPlayers(max_players));
        }
        Ok(Self {
            instance,
            application,
            max_players,
            current_players: 0,
            name: name.to_owned(),
            flags: SessionFlags::HOST_DEFAULT,
        })
    }
}

/// One advertised service provider (`DirectPlayEnumerateA`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub name: String,
}

/// One enumerated player (`EnumPlayers`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: PlayerName,
}

/// Message destination filters the recovered call sites use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageTarget {
    Player(PlayerId),
    AllPlayers,
    Server,
}

/// One received message (`Receive`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingMessage {
    pub from: PlayerId,
    pub to: PlayerId,
    pub payload: Vec<u8>,
}

/// The transport-side lifecycle the game drives.
///
/// Host and join are mutually exclusive per session object; `close` returns the
/// object to [`TransportState::Idle`] so a later session can be hosted or
/// joined again, exactly as the shipped teardown does.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TransportState {
    #[default]
    Idle,
    /// Providers enumerated; a session object may now be created.
    Ready,
    Hosting(SessionDesc),
    Joined(SessionDesc),
}

impl TransportState {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Ready => "ready",
            Self::Hosting(_) => "hosting",
            Self::Joined(_) => "joined",
        }
    }

    #[must_use]
    pub const fn is_in_session(&self) -> bool {
        matches!(self, Self::Hosting(_) | Self::Joined(_))
    }
}

/// A backend implementing the recovered session surface.
pub trait NetworkTransport {
    type Error;

    /// `DirectPlayEnumerateA`: the available service providers.
    fn enumerate_providers(&mut self) -> Result<Vec<ProviderInfo>, Self::Error>;

    /// `DirectPlayCreate` + `QueryInterface`: build the session object for one
    /// provider.
    fn create(&mut self, provider: ProviderId) -> Result<(), Self::Error>;

    /// `SetSessionDesc`: advertise this host's descriptor.
    fn set_session_desc(&mut self, desc: &SessionDesc) -> Result<(), Self::Error>;

    /// `Open`: enter the session. Host opens its own descriptor, a client opens
    /// the enumerated one.
    fn open(&mut self, desc: &SessionDesc, join_enabled: bool) -> Result<(), Self::Error>;

    /// `EnumSessions`: advertise sessions of the given application.
    fn enumerate_sessions(
        &mut self,
        application: ApplicationId,
    ) -> Result<Vec<SessionDesc>, Self::Error>;

    /// `CreatePlayer`: add the local player.
    fn create_player(&mut self, name: &PlayerName) -> Result<PlayerId, Self::Error>;

    /// `EnumPlayers`: current player list.
    fn enumerate_players(&mut self) -> Result<Vec<PlayerInfo>, Self::Error>;

    /// `DestroyPlayer`.
    fn destroy_player(&mut self, player: PlayerId) -> Result<(), Self::Error>;

    /// `Send`.
    fn send(
        &mut self,
        from: PlayerId,
        target: MessageTarget,
        payload: &[u8],
    ) -> Result<(), Self::Error>;

    /// `Receive` with the shipped size-probe semantics collapsed into an
    /// optional message.
    fn receive(&mut self) -> Result<Option<IncomingMessage>, Self::Error>;

    /// Release the session object and all player state.
    fn close(&mut self) -> Result<(), Self::Error>;
}

/// A wrong-order call on the recovered lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    /// A call that requires a live session arrived while idle/ready.
    NotInSession,
    /// A call that requires no session (create/host/join) arrived while one was
    /// live.
    AlreadyInSession,
    /// The provider list has not been enumerated.
    ProvidersNotEnumerated,
    /// The selected provider is not in the enumerated set.
    UnknownProvider,
    /// The selected session is not one of the enumerated sessions.
    UnknownSession,
    /// The session name is empty.
    EmptySessionName,
    /// The player name is empty.
    EmptyPlayerName,
    /// `max_players` is zero or above the recovered 12-slot table.
    InvalidMaxPlayers(u32),
    /// The session already has its local player.
    PlayerAlreadyCreated,
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInSession => write!(formatter, "no session is open"),
            Self::AlreadyInSession => write!(formatter, "a session is already open"),
            Self::ProvidersNotEnumerated => {
                write!(formatter, "service providers have not been enumerated")
            }
            Self::UnknownProvider => write!(formatter, "the selected provider is not advertised"),
            Self::UnknownSession => write!(formatter, "the selected session is not advertised"),
            Self::EmptySessionName => write!(formatter, "the session name must not be empty"),
            Self::EmptyPlayerName => write!(formatter, "the player name must not be empty"),
            Self::InvalidMaxPlayers(players) => write!(
                formatter,
                "max players {players} is outside 1..={MAX_SESSION_PLAYERS}"
            ),
            Self::PlayerAlreadyCreated => write!(formatter, "the local player already exists"),
        }
    }
}

impl std::error::Error for TransportError {}

/// A lifecycle ordering error or a backend failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkError<E> {
    /// The caller asked for a transition the shipped code never performs.
    Order(TransportError),
    /// The backend rejected the operation.
    Backend { operation: &'static str, source: E },
}

impl<E: fmt::Display> fmt::Display for NetworkError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Order(error) => write!(formatter, "{error}"),
            Self::Backend { operation, source } => {
                write!(formatter, "transport {operation} failed: {source}")
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for NetworkError<E> {}

/// Drives the recovered host/join/player/message sequence over any backend.
///
/// Every method enforces the shipped ordering and returns
/// [`TransportError`] without touching the backend when the caller asks for a
/// transition the original never performs.
pub struct SessionLifecycle {
    state: TransportState,
    providers: Vec<ProviderInfo>,
    local_player: Option<PlayerId>,
}

impl Default for SessionLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLifecycle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: TransportState::Idle,
            providers: Vec::new(),
            local_player: None,
        }
    }

    #[must_use]
    pub fn state(&self) -> &TransportState {
        &self.state
    }

    #[must_use]
    pub fn providers(&self) -> &[ProviderInfo] {
        &self.providers
    }

    #[must_use]
    pub const fn local_player(&self) -> Option<PlayerId> {
        self.local_player
    }

    /// `DirectPlayEnumerateA`, then `DirectPlayCreate` + QI for `provider`.
    ///
    /// Only valid while idle: the shipped code builds one session object at a
    /// time and releases it on teardown.
    pub fn start<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
        provider: ProviderId,
    ) -> Result<(), NetworkError<B::Error>> {
        if self.state.is_in_session() {
            return Err(NetworkError::Order(TransportError::AlreadyInSession));
        }
        let providers =
            transport
                .enumerate_providers()
                .map_err(|source| NetworkError::Backend {
                    operation: "enumerate_providers",
                    source,
                })?;
        if !providers.iter().any(|info| info.id == provider) {
            return Err(NetworkError::Order(TransportError::UnknownProvider));
        }
        self.providers = providers;
        transport
            .create(provider)
            .map_err(|source| NetworkError::Backend {
                operation: "create",
                source,
            })?;
        self.state = TransportState::Ready;
        Ok(())
    }

    /// Hosts a session: `SetSessionDesc`, `Open`, `CreatePlayer`,
    /// `EnumPlayers`, and a second `Open` that enables joins (the shipped host
    /// path opens with `DPSESSION_JOINDISABLED` toggled by an argument).
    pub fn host<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
        desc: SessionDesc,
        local_name: &PlayerName,
    ) -> Result<(), NetworkError<B::Error>> {
        self.require_ready()?;
        transport
            .set_session_desc(&desc)
            .map_err(|source| NetworkError::Backend {
                operation: "set_session_desc",
                source,
            })?;
        // The recovered host path opens once with joins disabled, creates its
        // player, then reopens with joins enabled.
        transport
            .open(&desc, false)
            .map_err(|source| NetworkError::Backend {
                operation: "open",
                source,
            })?;
        let player =
            transport
                .create_player(local_name)
                .map_err(|source| NetworkError::Backend {
                    operation: "create_player",
                    source,
                })?;
        self.local_player = Some(player);
        transport
            .enumerate_players()
            .map_err(|source| NetworkError::Backend {
                operation: "enumerate_players",
                source,
            })?;
        transport
            .open(&desc, true)
            .map_err(|source| NetworkError::Backend {
                operation: "open_joins",
                source,
            })?;
        self.state = TransportState::Hosting(desc);
        Ok(())
    }

    /// Joins an advertised session: `EnumSessions`, select, `Open`,
    /// `CreatePlayer`, `EnumPlayers`.
    pub fn join<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
        application: ApplicationId,
        session: SessionInstanceId,
        local_name: &PlayerName,
    ) -> Result<(), NetworkError<B::Error>> {
        self.require_ready()?;
        let sessions = transport
            .enumerate_sessions(application)
            .map_err(|source| NetworkError::Backend {
                operation: "enumerate_sessions",
                source,
            })?;
        let desc = sessions
            .into_iter()
            .find(|candidate| candidate.instance == session)
            .ok_or(NetworkError::Order(TransportError::UnknownSession))?;
        transport
            .open(&desc, true)
            .map_err(|source| NetworkError::Backend {
                operation: "open",
                source,
            })?;
        let player =
            transport
                .create_player(local_name)
                .map_err(|source| NetworkError::Backend {
                    operation: "create_player",
                    source,
                })?;
        self.local_player = Some(player);
        transport
            .enumerate_players()
            .map_err(|source| NetworkError::Backend {
                operation: "enumerate_players",
                source,
            })?;
        self.state = TransportState::Joined(desc);
        Ok(())
    }

    /// `Send` from the local player to a target.
    pub fn send<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
        target: MessageTarget,
        payload: &[u8],
    ) -> Result<(), NetworkError<B::Error>> {
        let local = self
            .local_player
            .ok_or(NetworkError::Order(TransportError::NotInSession))?;
        transport
            .send(local, target, payload)
            .map_err(|source| NetworkError::Backend {
                operation: "send",
                source,
            })
    }

    /// `Receive` (the shipped size-probe returns no message when empty).
    pub fn receive<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
    ) -> Result<Option<IncomingMessage>, NetworkError<B::Error>> {
        if !self.state.is_in_session() {
            return Err(NetworkError::Order(TransportError::NotInSession));
        }
        transport.receive().map_err(|source| NetworkError::Backend {
            operation: "receive",
            source,
        })
    }

    /// `DestroyPlayer` for the local player, then teardown.
    ///
    /// The shipped teardown also releases the interface and clears the network
    /// table; `close` models both. It is idempotent while idle.
    pub fn close<B: NetworkTransport>(
        &mut self,
        transport: &mut B,
    ) -> Result<(), NetworkError<B::Error>> {
        if let Some(player) = self.local_player.take() {
            transport
                .destroy_player(player)
                .map_err(|source| NetworkError::Backend {
                    operation: "destroy_player",
                    source,
                })?;
        }
        if self.state != TransportState::Idle {
            transport.close().map_err(|source| NetworkError::Backend {
                operation: "close",
                source,
            })?;
        }
        self.state = TransportState::Idle;
        self.providers.clear();
        Ok(())
    }

    fn require_ready<E>(&self) -> Result<(), NetworkError<E>> {
        if self.state.is_in_session() {
            return Err(NetworkError::Order(TransportError::AlreadyInSession));
        }
        if self.state != TransportState::Ready {
            return Err(NetworkError::Order(TransportError::ProvidersNotEnumerated));
        }
        Ok(())
    }
}

/// A deterministic in-memory backend implementing the recovered surface.
///
/// Used by contract tests and available to hosts that want a no-network
/// session (the object model is the recovered one; the transport is local).
#[derive(Debug, Default)]
pub struct LoopbackTransport {
    providers: Vec<ProviderInfo>,
    sessions: Vec<SessionDesc>,
    players: Vec<PlayerInfo>,
    next_player: u32,
    local_created: bool,
    open_flags: Vec<(SessionInstanceId, bool)>,
    sent: Vec<(PlayerId, MessageTarget, Vec<u8>)>,
    inbox: Vec<IncomingMessage>,
    closed: bool,
    created_provider: Option<ProviderId>,
}

impl LoopbackTransport {
    /// Builds a loopback with the given advertised providers.
    #[must_use]
    pub fn with_providers(providers: Vec<ProviderInfo>) -> Self {
        Self {
            providers,
            next_player: 2,
            ..Self::default()
        }
    }

    /// Advertises a session for the join path.
    pub fn advertise_session(&mut self, desc: SessionDesc) {
        self.sessions.push(desc);
    }

    /// Queues a message for the next `receive`.
    pub fn queue_message(&mut self, message: IncomingMessage) {
        self.inbox.push(message);
    }

    #[must_use]
    pub fn open_history(&self) -> &[(SessionInstanceId, bool)] {
        &self.open_flags
    }

    #[must_use]
    pub fn sent_messages(&self) -> &[(PlayerId, MessageTarget, Vec<u8>)] {
        &self.sent
    }

    #[must_use]
    pub fn created_provider(&self) -> Option<ProviderId> {
        self.created_provider
    }

    #[must_use]
    pub fn players(&self) -> &[PlayerInfo] {
        &self.players
    }

    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }
}

impl NetworkTransport for LoopbackTransport {
    type Error = TransportError;

    fn enumerate_providers(&mut self) -> Result<Vec<ProviderInfo>, Self::Error> {
        Ok(self.providers.clone())
    }

    fn create(&mut self, provider: ProviderId) -> Result<(), Self::Error> {
        self.created_provider = Some(provider);
        Ok(())
    }

    fn set_session_desc(&mut self, desc: &SessionDesc) -> Result<(), Self::Error> {
        if !self.sessions.iter().any(|s| s.instance == desc.instance) {
            self.sessions.push(desc.clone());
        }
        Ok(())
    }

    fn open(&mut self, desc: &SessionDesc, join_enabled: bool) -> Result<(), Self::Error> {
        self.open_flags.push((desc.instance, join_enabled));
        self.closed = false;
        Ok(())
    }

    fn enumerate_sessions(
        &mut self,
        application: ApplicationId,
    ) -> Result<Vec<SessionDesc>, Self::Error> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| s.application == application)
            .cloned()
            .collect())
    }

    fn create_player(&mut self, name: &PlayerName) -> Result<PlayerId, Self::Error> {
        if self.local_created {
            return Err(TransportError::PlayerAlreadyCreated);
        }
        let id = PlayerId(self.next_player);
        self.next_player += 1;
        self.local_created = true;
        self.players.push(PlayerInfo {
            id,
            name: name.clone(),
        });
        Ok(id)
    }

    fn enumerate_players(&mut self) -> Result<Vec<PlayerInfo>, Self::Error> {
        Ok(self.players.clone())
    }

    fn destroy_player(&mut self, player: PlayerId) -> Result<(), Self::Error> {
        self.players.retain(|info| info.id != player);
        self.local_created = false;
        Ok(())
    }

    fn send(
        &mut self,
        from: PlayerId,
        target: MessageTarget,
        payload: &[u8],
    ) -> Result<(), Self::Error> {
        self.sent.push((from, target, payload.to_vec()));
        Ok(())
    }

    fn receive(&mut self) -> Result<Option<IncomingMessage>, Self::Error> {
        if self.inbox.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.inbox.remove(0)))
        }
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        self.closed = true;
        self.players.clear();
        self.local_created = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> ProviderId {
        ProviderId([7; 16])
    }

    fn app() -> ApplicationId {
        ApplicationId([9; 16])
    }

    fn instance() -> SessionInstanceId {
        SessionInstanceId([3; 16])
    }

    fn transport() -> LoopbackTransport {
        LoopbackTransport::with_providers(vec![ProviderInfo {
            id: provider(),
            name: "TCP/IP".to_owned(),
        }])
    }

    fn ready() -> (SessionLifecycle, LoopbackTransport) {
        let mut lifecycle = SessionLifecycle::new();
        let mut transport = transport();
        lifecycle.start(&mut transport, provider()).unwrap();
        (lifecycle, transport)
    }

    #[test]
    fn host_follows_the_recovered_open_create_enumerate_reopen_order() {
        let (mut lifecycle, mut transport) = ready();
        let desc = SessionDesc::hosting(instance(), app(), "Rally", 6).unwrap();
        assert_eq!(desc.flags.bits(), 0x44);
        lifecycle
            .host(
                &mut transport,
                desc.clone(),
                &PlayerName::new("P1", "Player One").unwrap(),
            )
            .unwrap();

        assert_eq!(
            transport.open_history(),
            &[(instance(), false), (instance(), true)],
            "the shipped host opens with joins disabled, creates its player, then reopens"
        );
        assert_eq!(lifecycle.state(), &TransportState::Hosting(desc));
        assert_eq!(lifecycle.local_player(), Some(PlayerId(2)));
        assert_eq!(transport.players().len(), 1);
    }

    #[test]
    fn join_requires_an_advertised_session_and_opens_it() {
        let (mut lifecycle, mut transport) = ready();
        let desc = SessionDesc::hosting(instance(), app(), "Host", 4).unwrap();
        transport.advertise_session(desc.clone());

        lifecycle
            .join(
                &mut transport,
                app(),
                instance(),
                &PlayerName::new("P2", "Player Two").unwrap(),
            )
            .unwrap();
        assert_eq!(transport.open_history(), &[(instance(), true)]);
        assert_eq!(lifecycle.state(), &TransportState::Joined(desc));

        // Unknown sessions fail closed without opening anything.
        let (mut other, mut other_transport) = ready();
        let failure = other
            .join(
                &mut other_transport,
                app(),
                SessionInstanceId([0xee; 16]),
                &PlayerName::new("P3", "Player Three").unwrap(),
            )
            .unwrap_err();
        assert_eq!(failure, NetworkError::Order(TransportError::UnknownSession));
        assert!(other_transport.open_history().is_empty());
    }

    #[test]
    fn start_rejects_unknown_providers_and_reentry() {
        let mut lifecycle = SessionLifecycle::new();
        let mut transport = transport();
        let failure = lifecycle
            .start(&mut transport, ProviderId([0xaa; 16]))
            .unwrap_err();
        assert_eq!(
            failure,
            NetworkError::Order(TransportError::UnknownProvider)
        );
        assert_eq!(lifecycle.state(), &TransportState::Idle);

        lifecycle.start(&mut transport, provider()).unwrap();
        let desc = SessionDesc::hosting(instance(), app(), "Rally", 2).unwrap();
        lifecycle
            .host(
                &mut transport,
                desc,
                &PlayerName::new("P1", "Player One").unwrap(),
            )
            .unwrap();
        let failure = lifecycle.start(&mut transport, provider()).unwrap_err();
        assert_eq!(
            failure,
            NetworkError::Order(TransportError::AlreadyInSession)
        );
    }

    #[test]
    fn host_and_join_require_an_enumerated_provider() {
        let mut lifecycle = SessionLifecycle::new();
        let mut transport = transport();
        let desc = SessionDesc::hosting(instance(), app(), "Rally", 2).unwrap();
        let failure = lifecycle
            .host(&mut transport, desc, &PlayerName::new("P1", "").unwrap())
            .unwrap_err();
        assert_eq!(
            failure,
            NetworkError::Order(TransportError::ProvidersNotEnumerated)
        );
        assert_eq!(lifecycle.state(), &TransportState::Idle);
    }

    #[test]
    fn send_and_receive_require_a_live_session() {
        let (mut lifecycle, mut transport) = ready();
        let failure = lifecycle
            .send(&mut transport, MessageTarget::AllPlayers, b"hi")
            .unwrap_err();
        assert_eq!(failure, NetworkError::Order(TransportError::NotInSession));
        let failure = lifecycle.receive(&mut transport).unwrap_err();
        assert_eq!(failure, NetworkError::Order(TransportError::NotInSession));

        let desc = SessionDesc::hosting(instance(), app(), "Rally", 2).unwrap();
        lifecycle
            .host(&mut transport, desc, &PlayerName::new("P1", "").unwrap())
            .unwrap();
        lifecycle
            .send(&mut transport, MessageTarget::Player(PlayerId(5)), b"sync")
            .unwrap();
        assert_eq!(
            transport.sent_messages(),
            &[(
                PlayerId(2),
                MessageTarget::Player(PlayerId(5)),
                b"sync".to_vec()
            )]
        );

        assert_eq!(lifecycle.receive(&mut transport).unwrap(), None);
        let message = IncomingMessage {
            from: PlayerId(5),
            to: PlayerId(2),
            payload: vec![1, 2, 3],
        };
        transport.queue_message(message.clone());
        assert_eq!(lifecycle.receive(&mut transport).unwrap(), Some(message));
    }

    #[test]
    fn close_destroys_the_local_player_and_is_idempotent() {
        let (mut lifecycle, mut transport) = ready();
        let desc = SessionDesc::hosting(instance(), app(), "Rally", 2).unwrap();
        lifecycle
            .host(&mut transport, desc, &PlayerName::new("P1", "").unwrap())
            .unwrap();
        lifecycle.close(&mut transport).unwrap();
        assert!(transport.is_closed());
        assert!(transport.players().is_empty());
        assert_eq!(lifecycle.state(), &TransportState::Idle);
        // A second close while idle does not touch the backend again.
        lifecycle.close(&mut transport).unwrap();
        assert_eq!(lifecycle.state(), &TransportState::Idle);
        // The lifecycle can host again after teardown (the shipped object is
        // rebuilt by the next session).
        lifecycle.start(&mut transport, provider()).unwrap();
        assert_eq!(lifecycle.state(), &TransportState::Ready);
    }

    #[test]
    fn session_and_player_bounds_match_the_recovered_tables() {
        assert_eq!(MAX_SESSION_PLAYERS, 12);
        for players in [0, 13, u32::MAX] {
            assert_eq!(
                SessionDesc::hosting(instance(), app(), "Rally", players),
                Err(TransportError::InvalidMaxPlayers(players))
            );
        }
        assert_eq!(
            SessionDesc::hosting(instance(), app(), "", 2),
            Err(TransportError::EmptySessionName)
        );
        assert_eq!(
            PlayerName::new("", ""),
            Err(TransportError::EmptyPlayerName)
        );

        // The long/short names are truncated at the shipped `strncpy` bounds.
        let long = "x".repeat(40);
        let name = PlayerName::new(&long, &long).unwrap();
        assert_eq!(name.short.len(), PlayerName::MAX_SHORT);
        assert_eq!(name.long.len(), PlayerName::MAX_LONG);
        // Truncation stays on a character boundary.
        let unicode = "é".repeat(20);
        let name = PlayerName::new(&unicode, &unicode).unwrap();
        assert_eq!(name.short.len(), PlayerName::MAX_SHORT);
        assert!(name.short.chars().all(|c| c == 'é'));
        assert_eq!(SessionFlags::HOST_DEFAULT.bits(), 0x44);
    }
}
