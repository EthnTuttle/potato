use ext_config::ConfigError;
use roles_logic_sv2::{
    mining_sv2::{ExtendedExtranonce, NewExtendedMiningJob, SetCustomMiningJob},
    parsers::Mining,
};
use std::{convert::From, fmt::Debug, sync::MutexGuard};
use std::{fmt, sync::PoisonError};
use sv1_api::server_to_client::{Notify, SetDifficulty};

use stratum_common::bitcoin::util::uint::ParseLengthError;

pub type ProxyResult<'a, T> = core::result::Result<T, Error<'a>>;

#[derive(Debug)]
pub enum ChannelSendError<'a> {
    SubmitSharesExtended(
        async_channel::SendError<roles_logic_sv2::mining_sv2::SubmitSharesExtended<'a>>,
    ),
    SetNewPrevHash(async_channel::SendError<roles_logic_sv2::mining_sv2::SetNewPrevHash<'a>>),
    NewExtendedMiningJob(async_channel::SendError<NewExtendedMiningJob<'a>>),
    Notify(tokio::sync::broadcast::error::SendError<Notify<'a>>),
    V1Message(async_channel::SendError<sv1_api::Message>),
    General(String),
    Extranonce(async_channel::SendError<(ExtendedExtranonce, u32)>),
    SetCustomMiningJob(
        async_channel::SendError<roles_logic_sv2::mining_sv2::SetCustomMiningJob<'a>>,
    ),
    NewTemplate(
        async_channel::SendError<(
            roles_logic_sv2::template_distribution_sv2::SetNewPrevHash<'a>,
            Vec<u8>,
        )>,
    ),
}

#[derive(Debug)]
pub enum Error<'a> {
    VecToSlice32(Vec<u8>),
    /// Errors on bad CLI argument input.
    BadCliArgs,
    /// Errors on bad `serde_json` serialize/deserialize.
    BadSerdeJson(serde_json::Error),
    /// Errors on bad `config` TOML deserialize.
    BadConfigDeserialize(ConfigError),
    /// Errors from `binary_sv2` crate.
    BinarySv2(binary_sv2::Error),
    /// Errors on bad noise handshake.
    CodecNoise(codec_sv2::noise_sv2::Error),
    /// Errors from `framing_sv2` crate.
    FramingSv2(framing_sv2::Error),
    /// Errors on bad `TcpStream` connection.
    Io(std::io::Error),
    /// Errors due to invalid extranonce from upstream
    InvalidExtranonce(String),
    /// Errors on bad `String` to `int` conversion.
    ParseInt(std::num::ParseIntError),
    /// Errors from `roles_logic_sv2` crate.
    RolesSv2Logic(roles_logic_sv2::errors::Error),
    UpstreamIncoming(roles_logic_sv2::errors::Error),
    /// SV1 protocol library error
    V1Protocol(sv1_api::error::Error<'a>),
    #[allow(dead_code)]
    SubprotocolMining(String),
    // Locking Errors
    PoisonLock,
    // Channel Receiver Error
    ChannelErrorReceiver(async_channel::RecvError),
    TokioChannelErrorRecv(tokio::sync::broadcast::error::RecvError),
    // Channel Sender Errors
    ChannelErrorSender(ChannelSendError<'a>),
    Uint256Conversion(ParseLengthError),
    SetDifficultyToMessage(SetDifficulty),
    Infallible(std::convert::Infallible),
    // used to handle SV2 protocol error messages from pool
    #[allow(clippy::enum_variant_names)]
    Sv2ProtocolError(Mining<'a>),
    #[allow(clippy::enum_variant_names)]
    TargetError(roles_logic_sv2::errors::Error),
    Sv1MessageTooLong,
    MiningPoolError(PoolError),
}

impl fmt::Display for Error<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            BadCliArgs => write!(f, "Bad CLI arg input"),
            BadSerdeJson(ref e) => write!(f, "Bad serde json: `{:?}`", e),
            BadConfigDeserialize(ref e) => write!(f, "Bad `config` TOML deserialize: `{:?}`", e),
            BinarySv2(ref e) => write!(f, "Binary SV2 error: `{:?}`", e),
            CodecNoise(ref e) => write!(f, "Noise error: `{:?}", e),
            FramingSv2(ref e) => write!(f, "Framing SV2 error: `{:?}`", e),
            InvalidExtranonce(ref e) => write!(f, "Invalid Extranonce error: `{:?}", e),
            Io(ref e) => write!(f, "I/O error: `{:?}", e),
            ParseInt(ref e) => write!(f, "Bad convert from `String` to `int`: `{:?}`", e),
            RolesSv2Logic(ref e) => write!(f, "Roles SV2 Logic Error: `{:?}`", e),
            V1Protocol(ref e) => write!(f, "V1 Protocol Error: `{:?}`", e),
            SubprotocolMining(ref e) => write!(f, "Subprotocol Mining Error: `{:?}`", e),
            UpstreamIncoming(ref e) => write!(f, "Upstream parse incoming error: `{:?}`", e),
            PoisonLock => write!(f, "Poison Lock error"),
            ChannelErrorReceiver(ref e) => write!(f, "Channel receive error: `{:?}`", e),
            TokioChannelErrorRecv(ref e) => write!(f, "Channel receive error: `{:?}`", e),
            ChannelErrorSender(ref e) => write!(f, "Channel send error: `{:?}`", e),
            Uint256Conversion(ref e) => write!(f, "U256 Conversion Error: `{:?}`", e),
            SetDifficultyToMessage(ref e) => {
                write!(f, "Error converting SetDifficulty to Message: `{:?}`", e)
            }
            VecToSlice32(ref e) => write!(f, "Standard Error: `{:?}`", e),
            Infallible(ref e) => write!(f, "Infallible Error:`{:?}`", e),
            Sv2ProtocolError(ref e) => {
                write!(f, "Received Sv2 Protocol Error from upstream: `{:?}`", e)
            }
            TargetError(ref e) => {
                write!(f, "Impossible to get target from hashrate: `{:?}`", e)
            }
            Sv1MessageTooLong => {
                write!(f, "Received an sv1 message that is longer than max len")
            }
            MiningPoolError(pool_error) => write!(f, "Pool error: `{:?}`", pool_error),
        }
    }
}

impl From<binary_sv2::Error> for Error<'_> {
    fn from(e: binary_sv2::Error) -> Self {
        Error::BinarySv2(e)
    }
}

impl From<codec_sv2::noise_sv2::Error> for Error<'_> {
    fn from(e: codec_sv2::noise_sv2::Error) -> Self {
        Error::CodecNoise(e)
    }
}

impl From<framing_sv2::Error> for Error<'_> {
    fn from(e: framing_sv2::Error) -> Self {
        Error::FramingSv2(e)
    }
}

impl From<std::io::Error> for Error<'_> {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<std::num::ParseIntError> for Error<'_> {
    fn from(e: std::num::ParseIntError) -> Self {
        Error::ParseInt(e)
    }
}

impl From<roles_logic_sv2::errors::Error> for Error<'_> {
    fn from(e: roles_logic_sv2::errors::Error) -> Self {
        Error::RolesSv2Logic(e)
    }
}

impl From<serde_json::Error> for Error<'_> {
    fn from(e: serde_json::Error) -> Self {
        Error::BadSerdeJson(e)
    }
}

impl From<ConfigError> for Error<'_> {
    fn from(e: ConfigError) -> Self {
        Error::BadConfigDeserialize(e)
    }
}

impl<'a> From<sv1_api::error::Error<'a>> for Error<'a> {
    fn from(e: sv1_api::error::Error<'a>) -> Self {
        Error::V1Protocol(e)
    }
}

impl From<async_channel::RecvError> for Error<'_> {
    fn from(e: async_channel::RecvError) -> Self {
        Error::ChannelErrorReceiver(e)
    }
}

impl From<tokio::sync::broadcast::error::RecvError> for Error<'_> {
    fn from(e: tokio::sync::broadcast::error::RecvError) -> Self {
        Error::TokioChannelErrorRecv(e)
    }
}

//*** LOCK ERRORS ***
impl<T> From<PoisonError<T>> for Error<'_> {
    fn from(_e: PoisonError<T>) -> Self {
        Error::PoisonLock
    }
}

// *** CHANNEL SENDER ERRORS ***
impl<'a> From<async_channel::SendError<roles_logic_sv2::mining_sv2::SubmitSharesExtended<'a>>>
    for Error<'a>
{
    fn from(
        e: async_channel::SendError<roles_logic_sv2::mining_sv2::SubmitSharesExtended<'a>>,
    ) -> Self {
        Error::ChannelErrorSender(ChannelSendError::SubmitSharesExtended(e))
    }
}

impl<'a> From<async_channel::SendError<roles_logic_sv2::mining_sv2::SetNewPrevHash<'a>>>
    for Error<'a>
{
    fn from(e: async_channel::SendError<roles_logic_sv2::mining_sv2::SetNewPrevHash<'a>>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::SetNewPrevHash(e))
    }
}

impl<'a> From<tokio::sync::broadcast::error::SendError<Notify<'a>>> for Error<'a> {
    fn from(e: tokio::sync::broadcast::error::SendError<Notify<'a>>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::Notify(e))
    }
}

impl From<async_channel::SendError<sv1_api::Message>> for Error<'_> {
    fn from(e: async_channel::SendError<sv1_api::Message>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::V1Message(e))
    }
}

impl From<async_channel::SendError<(ExtendedExtranonce, u32)>> for Error<'_> {
    fn from(e: async_channel::SendError<(ExtendedExtranonce, u32)>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::Extranonce(e))
    }
}

impl<'a> From<async_channel::SendError<NewExtendedMiningJob<'a>>> for Error<'a> {
    fn from(e: async_channel::SendError<NewExtendedMiningJob<'a>>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::NewExtendedMiningJob(e))
    }
}

impl<'a> From<async_channel::SendError<SetCustomMiningJob<'a>>> for Error<'a> {
    fn from(e: async_channel::SendError<SetCustomMiningJob<'a>>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::SetCustomMiningJob(e))
    }
}

impl<'a>
    From<
        async_channel::SendError<(
            roles_logic_sv2::template_distribution_sv2::SetNewPrevHash<'a>,
            Vec<u8>,
        )>,
    > for Error<'a>
{
    fn from(
        e: async_channel::SendError<(
            roles_logic_sv2::template_distribution_sv2::SetNewPrevHash<'a>,
            Vec<u8>,
        )>,
    ) -> Self {
        Error::ChannelErrorSender(ChannelSendError::NewTemplate(e))
    }
}

impl From<Vec<u8>> for Error<'_> {
    fn from(e: Vec<u8>) -> Self {
        Error::VecToSlice32(e)
    }
}

impl From<ParseLengthError> for Error<'_> {
    fn from(e: ParseLengthError) -> Self {
        Error::Uint256Conversion(e)
    }
}

impl From<SetDifficulty> for Error<'_> {
    fn from(e: SetDifficulty) -> Self {
        Error::SetDifficultyToMessage(e)
    }
}

impl From<std::convert::Infallible> for Error<'_> {
    fn from(e: std::convert::Infallible) -> Self {
        Error::Infallible(e)
    }
}

impl<'a> From<Mining<'a>> for Error<'a> {
    fn from(e: Mining<'a>) -> Self {
        Error::Sv2ProtocolError(e)
    }
}

impl From<async_channel::SendError<()>> for Error<'_> {
    fn from(e: async_channel::SendError<()>) -> Self {
        Error::ChannelErrorSender(ChannelSendError::General(e.to_string()))
    }
}

impl From<async_channel::SendError<roles_logic_sv2::template_distribution_sv2::NewTemplate<'_>>>
    for Error<'_>
{
    fn from(
        e: async_channel::SendError<roles_logic_sv2::template_distribution_sv2::NewTemplate<'_>>,
    ) -> Self {
        Error::ChannelErrorSender(ChannelSendError::General(e.to_string()))
    }
}

impl
    From<async_channel::SendError<roles_logic_sv2::template_distribution_sv2::SetNewPrevHash<'_>>>
    for Error<'_>
{
    fn from(
        e: async_channel::SendError<roles_logic_sv2::template_distribution_sv2::SetNewPrevHash<'_>>,
    ) -> Self {
        Error::ChannelErrorSender(ChannelSendError::General(e.to_string()))
    }
}

#[derive(std::fmt::Debug)]
pub enum PoolError {
    Io(std::io::Error),
    ChannelSend(Box<dyn std::marker::Send + Debug>),
    ChannelRecv(async_channel::RecvError),
    BinarySv2(binary_sv2::Error),
    Codec(codec_sv2::Error),
    Noise(noise_sv2::Error),
    RolesLogic(roles_logic_sv2::Error),
    Framing(codec_sv2::framing_sv2::Error),
    PoisonLock(String),
    ComponentShutdown(String),
    Custom(String),
    Sv2ProtocolError((u32, Mining<'static>)),
}

impl From<PoolError> for Error<'static> {
    fn from(val: PoolError) -> Self {
        Error::MiningPoolError(val)
    }
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use PoolError::*;
        match self {
            Io(ref e) => write!(f, "I/O error: `{:?}", e),
            ChannelSend(ref e) => write!(f, "Channel send failed: `{:?}`", e),
            ChannelRecv(ref e) => write!(f, "Channel recv failed: `{:?}`", e),
            BinarySv2(ref e) => write!(f, "Binary SV2 error: `{:?}`", e),
            Codec(ref e) => write!(f, "Codec SV2 error: `{:?}", e),
            Framing(ref e) => write!(f, "Framing SV2 error: `{:?}`", e),
            Noise(ref e) => write!(f, "Noise SV2 error: `{:?}", e),
            RolesLogic(ref e) => write!(f, "Roles Logic SV2 error: `{:?}`", e),
            PoisonLock(ref e) => write!(f, "Poison lock: {:?}", e),
            ComponentShutdown(ref e) => write!(f, "Component shutdown: {:?}", e),
            Custom(ref e) => write!(f, "Custom SV2 error: `{:?}`", e),
            Sv2ProtocolError(ref e) => {
                write!(f, "Received Sv2 Protocol Error from upstream: `{:?}`", e)
            }
        }
    }
}

pub type PoolResult<T> = Result<T, PoolError>;

impl From<std::io::Error> for PoolError {
    fn from(e: std::io::Error) -> PoolError {
        PoolError::Io(e)
    }
}

impl From<async_channel::RecvError> for PoolError {
    fn from(e: async_channel::RecvError) -> PoolError {
        PoolError::ChannelRecv(e)
    }
}

impl From<binary_sv2::Error> for PoolError {
    fn from(e: binary_sv2::Error) -> PoolError {
        PoolError::BinarySv2(e)
    }
}

impl From<codec_sv2::Error> for PoolError {
    fn from(e: codec_sv2::Error) -> PoolError {
        PoolError::Codec(e)
    }
}

impl From<noise_sv2::Error> for PoolError {
    fn from(e: noise_sv2::Error) -> PoolError {
        PoolError::Noise(e)
    }
}

impl From<roles_logic_sv2::Error> for PoolError {
    fn from(e: roles_logic_sv2::Error) -> PoolError {
        PoolError::RolesLogic(e)
    }
}

impl<T: 'static + std::marker::Send + Debug> From<async_channel::SendError<T>> for PoolError {
    fn from(e: async_channel::SendError<T>) -> PoolError {
        PoolError::ChannelSend(Box::new(e))
    }
}

impl From<String> for PoolError {
    fn from(e: String) -> PoolError {
        PoolError::Custom(e)
    }
}
impl From<codec_sv2::framing_sv2::Error> for PoolError {
    fn from(e: codec_sv2::framing_sv2::Error) -> PoolError {
        PoolError::Framing(e)
    }
}

impl<T> From<PoisonError<MutexGuard<'_, T>>> for PoolError {
    fn from(e: PoisonError<MutexGuard<T>>) -> PoolError {
        PoolError::PoisonLock(e.to_string())
    }
}

impl From<(u32, Mining<'static>)> for PoolError {
    fn from(e: (u32, Mining<'static>)) -> Self {
        PoolError::Sv2ProtocolError(e)
    }
}
