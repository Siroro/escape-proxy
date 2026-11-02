//! Hostname matching and IDNA normalisation tests (beyond the unit tests in
//! `core::matcher`).

use escape_proxy::core::matcher::{normalize_domain, DomainList};

#[test]
fn normalisation_is_idempotent_and_sorting_dedupes() {
    let list = DomainList::new(["BAT.example", "bat.example", "a.example"]).unwrap();
    let entries = list.sorted();
    assert_eq!(entries, vec!["a.example".to_string(), "bat.example".to_string()]);
    // xn-- form is already normalised.
    assert_eq!(normalize_domain("xn--bcher-kva.example").unwrap(), "xn--bcher-kva.example");
}

#[test]
fn unicode_host_matches_punycode_entry() {
    let list = DomainList::new(["bücher.de"]).unwrap();
    assert!(list.allows("BÜCHER.DE"));
    assert!(list.allows("www.xn--bcher-kva.de"));
    assert!(!list.allows("bücher.de.evil.com"));
}

#[test]
fn deep_subdomains_match_but_siblings_do_not() {
    let list = DomainList::new(["google.com"]).unwrap();
    assert!(list.allows("a.b.c.google.com"));
    assert!(!list.allows("com"));
    assert!(!list.allows("google.uk"));
}

#[test]
fn hosts_that_fail_normalisation_are_rejected() {
    let list = DomainList::new(["example.com"]).unwrap();
    assert!(!list.allows("not a host"));
    assert!(!list.allows(""));
    assert!(!list.allows("192.168.0.1"));
}

#[test]
fn trailing_dot_is_tolerated() {
    let list = DomainList::new(["example.com."]).unwrap();
    assert_eq!(list.sorted(), vec!["example.com".to_string()]);
    assert!(list.allows("www.example.com."));
}
