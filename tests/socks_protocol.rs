//! SOCKS5 protocol parser tests: framing, malformed requests, replies.

use escape_proxy::core::socks::{
    self, error_reply, method_reply, parse_greeting, parse_request, success_reply, Address,
    Command, Reply, SocksError,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[test]
fn greeting_accepts_no_auth() {
    assert!(parse_greeting(&[5, 1], &[0]).is_ok());
    assert!(parse_greeting(&[5, 2], &[0, 2]).is_ok());
}

#[test]
fn greeting_rejects_bad_version_and_length() {
    assert!(matches!(parse_greeting(&[4, 1], &[0]), Err(SocksError::BadVersion(4))));
    // Declared 2 methods, only 1 supplied.
    assert!(matches!(parse_greeting(&[5, 2], &[0]), Err(SocksError::UnexpectedEof)));
}

#[test]
fn request_parses_ipv4() {
    let frame = [5, 1, 0, 1, 127, 0, 0, 1, 0x1f, 0x90];
    let (req, used) = parse_request(&frame).unwrap();
    assert_eq!(used, 10);
    assert_eq!(req.command, Command::Connect);
    assert_eq!(
        req.address,
        Address::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
    );
    assert_eq!(req.port, 8080);
}

#[test]
fn request_parses_domain() {
    let mut frame = vec![5, 1, 0, 3, 11];
    frame.extend_from_slice(b"example.com");
    frame.extend_from_slice(&443u16.to_be_bytes());
    let (req, used) = parse_request(&frame).unwrap();
    assert_eq!(used, 5 + 11 + 2);
    assert_eq!(req.address, Address::Domain("example.com".to_string()));
    assert_eq!(req.port, 443);
}

#[test]
fn request_parses_ipv6() {
    let mut frame = vec![5, 1, 0, 4];
    frame.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
    frame.extend_from_slice(&1080u16.to_be_bytes());
    let (req, used) = parse_request(&frame).unwrap();
    assert_eq!(used, 22);
    assert_eq!(req.address, Address::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
}

#[test]
fn request_rejects_bad_frames() {
    // Wrong version.
    assert!(matches!(parse_request(&[4, 1, 0, 1, 0, 0, 0, 0, 0, 0]), Err(SocksError::BadVersion(4))));
    // BIND command.
    assert!(matches!(parse_request(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]), Err(SocksError::BadCommand(2))));
    // UDP ASSOCIATE command.
    assert!(matches!(parse_request(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0]), Err(SocksError::BadCommand(3))));
    // Reserved byte set.
    assert!(matches!(parse_request(&[5, 1, 9, 1, 0, 0, 0, 0, 0, 0]), Err(SocksError::BadReserved(9))));
    // Unknown address type.
    assert!(matches!(parse_request(&[5, 1, 0, 9]), Err(SocksError::BadAddressType(9))));
    // Truncated frames.
    assert!(matches!(parse_request(&[5, 1, 0]), Err(SocksError::UnexpectedEof)));
    assert!(matches!(parse_request(&[5, 1, 0, 1, 127, 0, 0]), Err(SocksError::UnexpectedEof)));
    assert!(matches!(parse_request(&[5, 1, 0, 3, 11, b'e']), Err(SocksError::UnexpectedEof)));
    // Empty domain.
    assert!(matches!(parse_request(&[5, 1, 0, 3, 0, 0, 80]), Err(SocksError::InvalidDomain(_))));
}

#[test]
fn request_rejects_non_utf8_domain() {
    let mut frame = vec![5, 1, 0, 3, 4, 0xff, 0xfe, 0xfd, 0xfc];
    frame.extend_from_slice(&80u16.to_be_bytes());
    assert!(matches!(parse_request(&frame), Err(SocksError::InvalidDomain(_))));
}

#[test]
fn reply_layout() {
    assert_eq!(method_reply(0)[0..2], [5, 0]);
    assert_eq!(success_reply(), [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(error_reply(Reply::NotAllowed)[1], 2);
    assert_eq!(error_reply(Reply::CommandNotSupported)[1], 7);
}

#[tokio::test]
async fn framed_reader_roundtrip() {
    use tokio::io::{duplex, AsyncWriteExt};

    let (mut client, mut server) = duplex(64);
    client.write_all(&[5, 2, 0, 2]).await.unwrap();
    let methods = socks::read_greeting(&mut server).await.unwrap();
    assert_eq!(methods, vec![0, 2]);

    client.write_all(&[5, 1, 0, 1, 10, 0, 0, 5, 0x26, 0x2a]).await.unwrap();
    let request = socks::read_request(&mut server).await.unwrap();
    assert_eq!(
        request.address,
        Address::Ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))
    );
    assert_eq!(request.port, 9770);
}
