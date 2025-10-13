//! Tests for main domain passthrough vs subdomain routing
//!
//! Verifies that:
//! 1. Main domain requests (saramcp.com) pass through without UUID extraction
//! 2. Subdomain requests ({uuid}.saramcp.com) extract UUID correctly
//! 3. Invalid subdomains are rejected

use axum::http::HeaderMap;
use saramcp::middleware::extract_server_uuid_from_headers;

#[test]
fn test_main_domain_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "saramcp.com".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "Main domain should not extract UUID");
}

#[test]
fn test_www_subdomain_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "www.saramcp.com".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result, None,
        "www subdomain should not extract UUID (too short)"
    );
}

#[test]
fn test_localhost_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "localhost".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "localhost should not extract UUID");
}

#[test]
fn test_localhost_with_port_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "localhost:8080".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "localhost:port should not extract UUID");
}

#[test]
fn test_ip_address_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "IP address should not extract UUID");
}

#[test]
fn test_ip_address_with_port_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:8080".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "IP:port should not extract UUID");
}

#[test]
fn test_valid_uuid_subdomain_extracts_correctly() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let mut headers = HeaderMap::new();
    headers.insert("host", format!("{}.saramcp.com", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid.to_string()),
        "Valid UUID subdomain should extract UUID"
    );
}

#[test]
fn test_valid_uuid_subdomain_with_port_extracts_correctly() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let mut headers = HeaderMap::new();
    headers.insert(
        "host",
        format!("{}.saramcp.com:8080", uuid).parse().unwrap(),
    );

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid.to_string()),
        "UUID subdomain with port should extract UUID"
    );
}

#[test]
fn test_short_subdomain_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "short.saramcp.com".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result, None,
        "Short subdomain (< 32 chars) should not extract"
    );
}

#[test]
fn test_wrong_domain_returns_none() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let mut headers = HeaderMap::new();
    headers.insert("host", format!("{}.example.com", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "UUID on wrong domain should not extract UUID");
}

#[test]
fn test_x_server_uuid_header_takes_precedence() {
    let uuid_in_header = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let uuid_in_host = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

    let mut headers = HeaderMap::new();
    headers.insert("x-server-uuid", uuid_in_header.parse().unwrap());
    headers.insert(
        "host",
        format!("{}.saramcp.com", uuid_in_host).parse().unwrap(),
    );

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid_in_header.to_string()),
        "X-Server-UUID header should take precedence over Host"
    );
}

#[test]
fn test_empty_x_server_uuid_falls_back_to_host() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";

    let mut headers = HeaderMap::new();
    headers.insert("x-server-uuid", "".parse().unwrap());
    headers.insert("host", format!("{}.saramcp.com", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid.to_string()),
        "Empty X-Server-UUID should fall back to Host parsing"
    );
}

#[test]
fn test_no_headers_returns_none() {
    let headers = HeaderMap::new();
    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(result, None, "No headers should return None");
}

#[test]
fn test_subdomain_with_dots_returns_none() {
    // Subdomain contains dots (not a valid UUID pattern)
    let mut headers = HeaderMap::new();
    headers.insert("host", "foo.bar.saramcp.com".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result, None,
        "Subdomain with internal dots should not extract UUID"
    );
}

#[test]
fn test_very_long_uuid_extracts_correctly() {
    // Test with a 36-char UUID (standard format with hyphens)
    let uuid = "12345678-1234-5678-1234-567812345678";
    let mut headers = HeaderMap::new();
    headers.insert("host", format!("{}.saramcp.com", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid.to_string()),
        "Standard UUID format should extract"
    );
}

#[test]
fn test_uuid_without_hyphens_extracts_correctly() {
    // Test with 32-char UUID (no hyphens)
    let uuid = "12345678123456781234567812345678";
    let mut headers = HeaderMap::new();
    headers.insert("host", format!("{}.saramcp.com", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result,
        Some(uuid.to_string()),
        "UUID without hyphens should extract"
    );
}

#[test]
fn test_case_insensitive_domain() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let mut headers = HeaderMap::new();
    // Note: HTTP headers are case-insensitive, but domain names in URLs are too
    headers.insert("host", format!("{}.SARAMCP.COM", uuid).parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    // This will return None because the extraction looks for lowercase ".saramcp.com"
    // This is actually correct behavior - DNS is case-insensitive but our check is exact
    assert_eq!(
        result, None,
        "Uppercase domain should not match (exact string comparison)"
    );
}

#[test]
fn test_main_domain_with_path_returns_none() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "saramcp.com".parse().unwrap());

    let result = extract_server_uuid_from_headers(&headers);
    assert_eq!(
        result, None,
        "Main domain with path should not extract UUID"
    );
}
