//! SOCKS5 protocol handling: greeting, CONNECT request parsing and replies.
//!
//! Implements RFC 1928 for the no-authentication method and TCP CONNECT only;
//! BIND and UDP ASSOCIATE are rejected. Parsing functions are pure so they can
//! be unit-tested directly.

use std::io;
use std::net::IpAddr;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

pub const SOCKS_VERSION: u8 = 5;
pub const NO_AUTHENTICATION: u8 = 0x00;
pub const NO_ACCEPTABLE_METHODS: u8 = 0xFF;

/// RFC 1928 reply codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Succeeded = 0x00,
    GeneralFailure = 0x01,
    NotAllowed = 0x02,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    TtlExpired = 0x06,
    CommandNotSupported = 0x07,
    AddressTypeNotSupported = 0x08,
}

#[derive(Debug, Error)]
pub enum SocksError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("unexpected end of stream")]
    UnexpectedEof,
    #[error("unsupported SOCKS version {0}")]
    BadVersion(u8),
    #[error("unsupported command {0}")]
    BadCommand(u8),
    #[error("reserved byte must be 0, got {0}")]
    BadReserved(u8),
    #[error("unsupported address type {0}")]
    BadAddressType(u8),
    #[error("invalid domain name: {0}")]
    InvalidDomain(String),
}

impl SocksError {
    /// The reply to send for this error, if the connection is still far
    /// enough along the protocol for an error reply to be meaningful.
    pub fn sendable_reply(&self) -> Option<Reply> {
        match self {
            SocksError::BadCommand(_) => Some(Reply::CommandNotSupported),
            SocksError::BadAddressType(_) | SocksError::InvalidDomain(_) => {
                Some(Reply::AddressTypeNotSupported)
            }
            _ => None,
        }
    }
}

/// Maps a connection failure to the closest SOCKS5 reply code.
pub fn io_error_reply(e: &io::Error) -> Reply {
    match e.kind() {
        io::ErrorKind::ConnectionRefused => Reply::ConnectionRefused,
        io::ErrorKind::TimedOut => Reply::TtlExpired,
        io::ErrorKind::NetworkUnreachable => Reply::NetworkUnreachable,
        io::ErrorKind::HostUnreachable => Reply::HostUnreachable,
        _ => Reply::GeneralFailure,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    Ip(IpAddr),
    Domain(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksRequest {
    pub command: Command,
    pub address: Address,
    pub port: u16,
}

/// Validates a method negotiation greeting. `methods.len()` must equal the
/// declared method count.
pub fn parse_greeting(header: &[u8; 2], methods: &[u8]) -> Result<(), SocksError> {
    if header[0] != SOCKS_VERSION {
        return Err(SocksError::BadVersion(header[0]));
    }
    if methods.len() != header[1] as usize {
        return Err(SocksError::UnexpectedEof);
    }
    Ok(())
}

/// Parses a complete request frame. Returns the request and the number of
/// bytes consumed.
pub fn parse_request(buf: &[u8]) -> Result<(SocksRequest, usize), SocksError> {
    if buf.len() < 4 {
        return Err(SocksError::UnexpectedEof);
    }
    if buf[0] != SOCKS_VERSION {
        return Err(SocksError::BadVersion(buf[0]));
    }
    let command = match buf[1] {
        0x01 => Command::Connect,
        other => return Err(SocksError::BadCommand(other)),
    };
    if buf[2] != 0 {
        return Err(SocksError::BadReserved(buf[2]));
    }
    match buf[3] {
        0x01 => {
            if buf.len() < 10 {
                return Err(SocksError::UnexpectedEof);
            }
            let ip = IpAddr::V4(std::net::Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]));
            let port = u16::from_be_bytes([buf[8], buf[9]]);
            Ok((SocksRequest { command, address: Address::Ip(ip), port }, 10))
        }
        0x03 => {
            if buf.len() < 5 {
                return Err(SocksError::UnexpectedEof);
            }
            let len = buf[4] as usize;
            if len == 0 {
                return Err(SocksError::InvalidDomain("empty".into()));
            }
            if buf.len() < 5 + len + 2 {
                return Err(SocksError::UnexpectedEof);
            }
            let host = std::str::from_utf8(&buf[5..5 + len])
                .map_err(|_| SocksError::InvalidDomain("not UTF-8".into()))?;
            let port = u16::from_be_bytes([buf[5 + len], buf[6 + len]]);
            Ok((
                SocksRequest {
                    command,
                    address: Address::Domain(host.to_string()),
                    port,
                },
                7 + len,
            ))
        }
        0x04 => {
            if buf.len() < 22 {
                return Err(SocksError::UnexpectedEof);
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&buf[4..20]);
            let ip = IpAddr::V6(std::net::Ipv6Addr::from(octets));
            let port = u16::from_be_bytes([buf[20], buf[21]]);
            Ok((SocksRequest { command, address: Address::Ip(ip), port }, 22))
        }
        other => Err(SocksError::BadAddressType(other)),
    }
}

pub fn method_reply(method: u8) -> [u8; 2] {
    [SOCKS_VERSION, method]
}

pub fn success_reply() -> [u8; 10] {
    [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]
}

pub fn error_reply(code: Reply) -> [u8; 10] {
    [5, code as u8, 0, 1, 0, 0, 0, 0, 0, 0]
}

/// Reads and validates the method negotiation greeting.
pub async fn read_greeting<R>(r: &mut R) -> Result<Vec<u8>, SocksError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 2];
    r.read_exact(&mut header).await?;
    let mut methods = vec![0u8; header[1] as usize];
    r.read_exact(&mut methods).await?;
    parse_greeting(&header, &methods)?;
    Ok(methods)
}

/// Reads and validates a CONNECT request.
pub async fn read_request<R>(r: &mut R) -> Result<SocksRequest, SocksError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 4];
    r.read_exact(&mut header).await?;
    let mut buf = header.to_vec();
    match header[3] {
        0x01 => {
            let mut rest = [0u8; 6];
            r.read_exact(&mut rest).await?;
            buf.extend_from_slice(&rest);
        }
        0x03 => {
            let mut len = [0u8; 1];
            r.read_exact(&mut len).await?;
            buf.extend_from_slice(&len);
            let mut rest = vec![0u8; len[0] as usize + 2];
            r.read_exact(&mut rest).await?;
            buf.extend_from_slice(&rest);
        }
        0x04 => {
            let mut rest = [0u8; 18];
            r.read_exact(&mut rest).await?;
            buf.extend_from_slice(&rest);
        }
        other => return Err(SocksError::BadAddressType(other)),
    }
    let (request, _) = parse_request(&buf)?;
    Ok(request)
}
