//! Shared HTTP proxy selection and connection pools for Sunrise downloads.
//!
//! Local hosts and hosts in `NO_PROXY` go direct. Other URLs use `HTTPS_PROXY` or `HTTP_PROXY`
//! for the URL's scheme, then `ALL_PROXY`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::net::Ipv6Addr;
use std::num::NonZeroU16;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, Response};
use url::Url;

type Agents = HashMap<Option<String>, Client>;

/// A fresh client starts a new connection pool. The proxy is the only setting that varies by client;
/// timeouts and headers belong to individual requests.
static KEPT: LazyLock<Mutex<Agents>> = LazyLock::new(Mutex::default);

/// A configuration error that can be reported without showing the proxy address or credentials.
#[derive(Debug)]
struct ProxyError {
    setting: &'static str,
    kind: InvalidProxy,
}

#[derive(Debug, PartialEq, Eq)]
enum InvalidProxy {
    Authority,
    Scheme,
    Encoding,
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.kind {
            InvalidProxy::Authority => "invalid proxy authority",
            InvalidProxy::Scheme => "unsupported proxy scheme",
            InvalidProxy::Encoding => "proxy value is not valid Unicode",
        };
        write!(f, "{}: {reason}", self.setting)
    }
}

impl std::error::Error for ProxyError {}

/// Which redirect targets a GET may contact.
#[derive(Clone, Copy)]
pub enum Redirects<'a> {
    /// Follow HTTP(S) redirects.
    Follow,
    /// Only use these exact HTTPS hosts on the default port, including the initial request.
    HttpsHosts(&'a [&'a str]),
    /// Return the first response, even if it is a redirect.
    None,
}

/// Download a response, choosing the proxy again before every permitted redirect. HTTP error
/// statuses are returned as responses so callers can handle cases such as 404 themselves.
pub fn get(
    url: &str,
    headers: &[(&str, &str)],
    redirects: Redirects<'_>,
    timeout: Option<Duration>,
) -> Result<Response, String> {
    request(url, headers, None, redirects, timeout)
}

/// Send a POST. Follow 301/302/303 as GET, but never resend its body to a redirect target.
pub fn post(
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
    timeout: Option<Duration>,
) -> Result<Response, String> {
    request(url, headers, Some(body), Redirects::Follow, timeout)
}

fn request(
    url: &str,
    headers: &[(&str, &str)],
    mut body: Option<Vec<u8>>,
    redirects: Redirects<'_>,
    timeout: Option<Duration>,
) -> Result<Response, String> {
    let start = Instant::now();
    follow(url, redirects, body.is_some(), |current, first| {
        let client = client_for(current)?;
        let mut request = match body.take() {
            Some(body) => client.post(current.clone()).body(body),
            None => client.get(current.clone()),
        };
        for &(name, value) in headers {
            // Don't forward authentication, cookies or entity headers on a redirect.
            if first
                || ["accept", "user-agent"]
                    .iter()
                    .any(|h| name.eq_ignore_ascii_case(h))
            {
                request = request.header(name, value);
            }
        }
        if let Some(timeout) = timeout {
            let remaining = timeout
                .checked_sub(start.elapsed())
                .ok_or("HTTP request timed out")?;
            request = request.timeout(remaining);
        }
        request.send().map_err(|e| e.without_url().to_string())
    })
}

fn follow(
    url: &str,
    redirects: Redirects<'_>,
    mut posting: bool,
    mut send: impl FnMut(&Url, bool) -> Result<Response, String>,
) -> Result<Response, String> {
    const MAX_REDIRECTS: usize = 5;
    let mut current = Url::parse(url).map_err(|_| "Invalid HTTP URL")?;
    for hop in 0..=MAX_REDIRECTS {
        if let Redirects::HttpsHosts(hosts) = redirects
            && (current.scheme() != "https"
                || !current.host_str().is_some_and(|host| hosts.contains(&host))
                || current.port().is_some()
                || !current.username().is_empty()
                || current.password().is_some())
        {
            return Err("Refused download URL outside allowed HTTPS hosts".into());
        }
        if !matches!(current.scheme(), "http" | "https") {
            return Err("Refused non-HTTP URL".into());
        }
        let response = send(&current, hop == 0)?;
        let follow = match redirects {
            Redirects::None => false,
            Redirects::HttpsHosts(_) => response.status().is_redirection(),
            Redirects::Follow => match response.status().as_u16() {
                301..=303 => true,
                307 | 308 => !posting,
                _ => false,
            },
        };
        if !follow {
            return Ok(response);
        }
        let Some(location) = response.headers().get("Location") else {
            return if matches!(redirects, Redirects::HttpsHosts(_)) {
                Err("Redirect without Location".into())
            } else {
                Ok(response)
            };
        };
        if hop == MAX_REDIRECTS {
            break;
        }
        let location = location.to_str().map_err(|_| "Invalid redirect Location")?;
        current = current
            .join(location)
            .map_err(|_| "Invalid redirect Location")?;
        posting = false;
    }
    Err("Too many redirects".into())
}

fn client_for(url: &Url) -> Result<Client, String> {
    let selected = if bypasses_proxy(url, &no_proxy()) {
        None
    } else {
        env_proxy(url.scheme(), |name| std::env::var_os(name)).map_err(|e| e.to_string())?
    };
    let mut agents = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    keep(&mut agents, selected, |proxy| {
        let builder = Client::builder()
            .no_proxy()
            .user_agent("sunrise-tools")
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .timeout(None);
        match proxy {
            Some(proxy) => builder.proxy(proxy),
            None => builder,
        }
        .build()
        .map_err(|e| format!("could not initialize HTTP client: {}", e.without_url()))
    })
}

/// Testable with a local pool. Only an absent setting requests a direct client.
fn keep(
    agents: &mut Agents,
    selected: Option<(&'static str, String)>,
    build: impl FnOnce(Option<reqwest::Proxy>) -> Result<Client, String>,
) -> Result<Client, String> {
    let (setting, proxy) = selected.map_or(("", None), |(name, spec)| (name, Some(spec)));
    match agents.entry(proxy) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let proxy = entry
                .key()
                .as_deref()
                .map(parse_proxy)
                .transpose()
                .map_err(|kind| ProxyError { setting, kind }.to_string())?;
            Ok(entry.insert(build(proxy)?).clone())
        }
    }
}

fn parse_proxy(spec: &str) -> Result<reqwest::Proxy, InvalidProxy> {
    let spec = spec.trim_end_matches('/');
    let (scheme, authority) = spec.split_once("://").unwrap_or(("http", spec));
    let scheme = match scheme {
        "http" | "socks4" | "socks4a" | "socks5h" => scheme,
        // Both spellings already resolved host names at the proxy under the old transport.
        "socks" | "socks5" => "socks5h",
        scheme if valid_scheme(scheme) => return Err(InvalidProxy::Scheme),
        _ => return Err(InvalidProxy::Authority),
    };
    let (credentials, address) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(creds, addr)| (Some(creds), addr));
    let (host, port) = address
        .split_once(':')
        .map_or((address, None), |(host, port)| (host, Some(port)));
    if host.is_empty()
        || host
            .chars()
            .any(|c| c.is_whitespace() || "[]/\\?#".contains(c))
        || authority.chars().any(char::is_control)
        || port.is_some_and(|p| {
            !p.bytes().all(|b| b.is_ascii_digit()) || p.parse::<NonZeroU16>().is_err()
        })
    {
        return Err(InvalidProxy::Authority);
    }
    let mut url =
        Url::parse(&format!("{scheme}://{address}")).map_err(|_| InvalidProxy::Authority)?;
    if let Some(credentials) = credentials {
        let (user, password) = credentials.split_once(':').ok_or(InvalidProxy::Authority)?;
        // The environment setting accepts raw userinfo; only the backend expects URL encoding.
        url.set_username(&user.replace('%', "%25"))
            .map_err(|()| InvalidProxy::Authority)?;
        url.set_password(Some(&password.replace('%', "%25")))
            .map_err(|()| InvalidProxy::Authority)?;
    }
    reqwest::Proxy::all(url).map_err(|_| InvalidProxy::Authority)
}

fn valid_scheme(scheme: &str) -> bool {
    matches!(scheme.as_bytes(), [first, rest @ ..] if first.is_ascii_alphabetic()
        && rest.iter().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(b)))
}

fn env_proxy(
    scheme: &str,
    mut read: impl FnMut(&str) -> Option<OsString>,
) -> Result<Option<(&'static str, String)>, ProxyError> {
    let names = if scheme.eq_ignore_ascii_case("http") {
        ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    };
    names
        .into_iter()
        .find_map(|name| {
            let value = read(name)?;
            let Some(value) = value.to_str() else {
                return Some(Err(ProxyError {
                    setting: name,
                    kind: InvalidProxy::Encoding,
                }));
            };
            let value = value.trim();
            (!value.is_empty()).then(|| Ok((name, value.to_owned())))
        })
        .transpose()
}

/// `NO_PROXY`'s list: the hosts reached directly whatever the proxy is.
fn no_proxy() -> String {
    ["NO_PROXY", "no_proxy"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .unwrap_or_default()
}

/// Whether `host` is this computer: the game's own API and a server on the same machine.
fn local(host: &str) -> bool {
    let host = bare(host);
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host == "0.0.0.0"
        || is_loopback_v4(&host)
}

/// A dotted IPv4 address in `127.0.0.0/8`; a DNS name that merely starts with `127.` is not one.
fn is_loopback_v4(host: &str) -> bool {
    host.starts_with("127.")
        && host
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether the destination is local or in `NO_PROXY`: `*`, `host`, `.domain`, `*.domain` and
/// `host:port` entries (matching the effective port, including the scheme's default).
fn bypasses_proxy(url: &Url, no_proxy: &str) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if local(host) {
        return true;
    }
    let host = bare(host);
    let port = url.port_or_known_default();
    no_proxy
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let entry = entry.trim_start_matches('*').trim_start_matches('.');
            // A port follows the last colon of a bracketed entry, or of a host with no colon
            // before it; an unbracketed IPv6 literal keeps its colons.
            let entry = match entry.rfind(':') {
                Some(at)
                    if !entry[at..].contains(']')
                        && (entry[..at].ends_with(']') || !entry[..at].contains(':')) =>
                {
                    if !entry[at + 1..]
                        .parse::<u16>()
                        .is_ok_and(|required| Some(required) == port)
                    {
                        return false;
                    }
                    &entry[..at]
                }
                _ => entry,
            };
            let entry = bare(entry);
            !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
        })
}

/// A lower-case host name or canonical IP literal, without IPv6 brackets or a DNS root dot.
fn bare(host: &str) -> String {
    let host = host.trim();
    let host = host
        .strip_suffix('.')
        .unwrap_or(host)
        .trim_matches(['[', ']']);
    if host.contains(':')
        && let Ok(ip) = host.parse::<Ipv6Addr>()
    {
        return ip.to_string();
    }
    host.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn this_computer_is_never_proxied() {
        for host in [
            "localhost",
            "LOCALHOST",
            "[::1]",
            "::1",
            "127.0.0.1",
            "127.1.2.3",
        ] {
            assert!(local(host), "{host}");
        }
        for host in [
            "playsunrise.net",
            "api.minecraftservices.com",
            "10.0.0.1",
            "127.example.com",
            "127.0.0.1.example.com",
        ] {
            assert!(!local(host), "{host}");
        }
    }

    #[test]
    fn proxy_environment_prefers_scheme_over_all_and_uppercase_over_lowercase() {
        let mut values = HashMap::from([
            ("HTTP_PROXY", "http://upper.example"),
            ("http_proxy", "http://lower.example"),
            ("https_proxy", " socks5h://secure.example "),
            ("ALL_PROXY", "http://all.example"),
            ("all_proxy", "http://all-lower.example"),
        ]);
        let selected = |scheme, values: &HashMap<&str, &str>| {
            env_proxy(scheme, |name| values.get(name).map(OsString::from))
                .unwrap()
                .map(|(_, value)| value)
        };
        assert_eq!(
            selected("http", &values).as_deref(),
            Some("http://upper.example")
        );
        assert_eq!(
            selected("https", &values).as_deref(),
            Some("socks5h://secure.example")
        );
        values.insert("HTTPS_PROXY", "http://secure-upper.example");
        assert_eq!(
            selected("https", &values).as_deref(),
            Some("http://secure-upper.example")
        );
        values.insert("HTTP_PROXY", " ");
        assert_eq!(
            selected("http", &values).as_deref(),
            Some("http://lower.example")
        );
        values.remove("http_proxy");
        assert_eq!(
            selected("http", &values).as_deref(),
            Some("http://all.example")
        );
        values.remove("ALL_PROXY");
        assert_eq!(
            selected("http", &values).as_deref(),
            Some("http://all-lower.example")
        );
        values.remove("all_proxy");
        assert_eq!(selected("http", &values), None);
    }

    fn bypasses(url: &str, no_proxy: &str) -> bool {
        bypasses_proxy(&Url::parse(url).unwrap(), no_proxy)
    }

    #[test]
    fn no_proxy_lists_hosts_and_their_subdomains() {
        assert!(bypasses("https://anything.at.all", "*"));
        assert!(bypasses("https://cdn.modrinth.com", "modrinth.com"));
        assert!(bypasses("https://api.modrinth.com", "MODRINTH.com"));
        assert!(bypasses("https://piston-meta.mojang.com", "*.mojang.com"));
        assert!(bypasses(
            "https://wiki.corp.example",
            " .corp.example , other.host "
        ));
        assert!(!bypasses("https://notmodrinth.com", "modrinth.com"));
        assert!(!bypasses(
            "https://modrinth.com",
            " .corp.example , mojang.com "
        ));
        assert!(!bypasses("https://edge.forgecdn.net", "modrinth.com"));
        assert!(!bypasses("https://modrinth.com", ""));
        assert!(!bypasses("http://192.0.2.10/", "192.0.2.0/24"));
    }

    #[test]
    fn a_dns_root_dot_does_not_change_the_route() {
        assert!(bypasses("https://localhost./", ""));
        assert!(bypasses("https://tools.localhost./", ""));
        assert!(bypasses("https://cdn.example.org./", "example.org"));
        assert!(bypasses("https://cdn.example.org/", ".EXAMPLE.ORG."));
        assert!(bypasses(
            "https://cdn.example.org.:8443/",
            "example.org.:8443"
        ));
        assert!(!bypasses("https://cdn.example.org./", "example.org.:8443"));
        assert!(!bypasses("https://notexample.org./", "example.org."));
    }

    #[test]
    fn no_proxy_requires_the_specified_destination_port() {
        assert!(bypasses(
            "https://mirror.example:8443/",
            "mirror.example:8443"
        ));
        assert!(!bypasses("https://mirror.example/", "mirror.example:8443"));
        assert!(bypasses("https://mirror.example/", "mirror.example:443"));
        assert!(!bypasses("http://mirror.example/", "mirror.example:443"));
        assert!(bypasses("http://mirror.example/", "mirror.example:80"));
        assert!(!bypasses("http://mirror.example/", "mirror.example:http"));
        assert!(bypasses("https://mirror.example:8443/", "mirror.example"));
        assert!(!bypasses(
            r"https://other.example\@mirror.example:8443/",
            "mirror.example:8443"
        ));
    }

    #[test]
    fn no_proxy_matches_ipv6_literals_with_or_without_brackets() {
        let url = "http://[2001:0db8::1]:8080/";
        assert!(bypasses(url, "2001:db8::1"));
        assert!(bypasses(url, "[2001:db8::1]"));
        assert!(bypasses(url, "[2001:0db8::1]:8080"));
        assert!(!bypasses(url, "[2001:db8::1]:8081"));
        assert!(!bypasses(url, "2001:db8::2"));
    }

    #[test]
    fn a_selected_invalid_proxy_never_builds_a_direct_client() {
        let mut agents = Agents::new();
        // An existing direct agent must not be reused when selection produces an invalid proxy.
        keep(&mut agents, None, |_| {
            Client::builder()
                .no_proxy()
                .build()
                .map_err(|e| e.to_string())
        })
        .unwrap();
        for (spec, kind) in [
            (
                "http://proxy.example:dummy-password",
                InvalidProxy::Authority,
            ),
            ("http://proxy.example:65536", InvalidProxy::Authority),
            ("http://proxy.example:", InvalidProxy::Authority),
            ("http://:3128", InvalidProxy::Authority),
            ("http://proxy.example/path", InvalidProxy::Authority),
            (
                "dummy-user:dummy-password://with@delimiters@proxy.example",
                InvalidProxy::Authority,
            ),
            (
                "https://dummy-user:dummy-password@proxy.example",
                InvalidProxy::Scheme,
            ),
        ] {
            let fail = |_| panic!("an invalid proxy must not be used to build an agent");
            let error = keep(&mut agents, Some(("HTTP_PROXY", spec.into())), fail).unwrap_err();
            assert_eq!(
                error,
                ProxyError {
                    setting: "HTTP_PROXY",
                    kind
                }
                .to_string()
            );
            let message = error;
            assert!(!message.contains("dummy-user"));
            assert!(!message.contains("dummy-password"));
        }
        for spec in [
            "http://127.0.0.1:7890",
            "127.0.0.1:7890",
            "dummy-user:dummy-password@proxy.example:3128",
            "socks5://127.0.0.1:7891",
        ] {
            assert!(parse_proxy(spec).is_ok());
        }
    }

    fn response(status: u16, location: Option<&str>) -> Response {
        let builder = http::Response::builder().status(status);
        let builder = match location {
            Some(location) => builder.header("Location", location),
            None => builder,
        };
        builder.body("").unwrap().into()
    }

    #[test]
    fn download_allowlist_is_checked_before_contacting_each_hop() {
        let allowed =
            Redirects::HttpsHosts(&["github.com", "release-assets.githubusercontent.com"]);
        let mut contacted = Vec::new();
        let refused = follow("https://github.com/file", allowed, false, |url, _| {
            contacted.push(url.to_string());
            let next = match contacted.len() {
                1 => "/release",
                2 => "https://release-assets.githubusercontent.com/file",
                _ => "https://unlisted.invalid/",
            };
            Ok(response(302, Some(next)))
        });
        assert_eq!(
            refused.unwrap_err(),
            "Refused download URL outside allowed HTTPS hosts"
        );
        assert_eq!(
            contacted,
            [
                "https://github.com/file",
                "https://github.com/release",
                "https://release-assets.githubusercontent.com/file",
            ]
        );
        for url in [
            "http://github.com/file",
            "https://github.com.invalid/",
            "https://u@github.com/file",
            "https://github.com:444/file",
        ] {
            assert!(follow(url, allowed, false, |_, _| panic!("must not contact {url}")).is_err());
        }
    }

    #[test]
    fn redirects_stop_at_the_limit() {
        let mut contacted = 0;
        let result = follow(
            "https://origin.invalid/",
            Redirects::Follow,
            false,
            |_, _| {
                contacted += 1;
                Ok(response(302, Some("/again")))
            },
        );
        assert_eq!(result.unwrap_err(), "Too many redirects");
        assert_eq!(contacted, 6);
    }

    #[test]
    fn release_discovery_returns_a_redirect_without_following_it() {
        let result = follow(
            "https://origin.invalid/",
            Redirects::None,
            false,
            |_, first| {
                assert!(
                    first,
                    "authenticated discovery must not make a second request"
                );
                Ok(response(302, Some("https://other.invalid/")))
            },
        )
        .unwrap();
        assert_eq!(result.status(), 302);
    }

    #[test]
    fn a_post_only_follows_redirects_that_switch_to_get() {
        for status in [307, 308] {
            let result = follow(
                "https://origin.invalid/",
                Redirects::Follow,
                true,
                |_, first| {
                    assert!(first, "must not replay the POST at the new URL");
                    Ok(response(status, Some("/other")))
                },
            )
            .unwrap();
            assert_eq!(result.status(), status);
        }
        let result = follow(
            "https://origin.invalid/",
            Redirects::Follow,
            true,
            |url, first| {
                if first {
                    return Ok(response(303, Some("/get")));
                }
                if url.path() == "/get" {
                    return Ok(response(307, Some("/final")));
                }
                assert_eq!(url.path(), "/final");
                Ok(response(200, None))
            },
        )
        .unwrap();
        assert_eq!(result.status(), 200);
    }

    /// A stand-in SOCKS5 proxy: answers one request itself and returns the host it was asked for.
    fn socks5_proxy() -> (u16, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut greeting = [0; 2];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting[0], 5);
            let mut methods = vec![0; greeting[1].into()];
            stream.read_exact(&mut methods).unwrap();
            stream.write_all(&[5, 0]).unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(request[..4], [5, 1, 0, 3], "a CONNECT to a host name");
            let mut host = vec![0; request[4].into()];
            stream.read_exact(&mut host).unwrap();
            let mut target_port = [0; 2];
            stream.read_exact(&mut target_port).unwrap();
            stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            let mut http = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while http.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            String::from_utf8(host).unwrap()
        });
        (port, server)
    }

    fn read_http_head(stream: &std::net::TcpStream) -> String {
        use std::io::{BufRead, BufReader};
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let n = reader.read_line(&mut head).unwrap();
            if n == 2 && head.ends_with("\r\n\r\n") {
                return head;
            }
            assert_ne!(n, 0, "client closed before the end of its HTTP headers");
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_refused_connect_is_a_transport_error() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_head(&stream);
            stream
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            request
        });
        let client = Client::builder()
            .no_proxy()
            .proxy(parse_proxy(&proxy).unwrap())
            .timeout(TIMEOUT)
            .build()
            .unwrap();
        let error = client
            .get("https://sunrise.invalid/payload")
            .send()
            .unwrap_err();
        assert!(error.is_connect(), "{error}");
        assert_eq!(
            server.join().unwrap().lines().next(),
            Some("CONNECT sunrise.invalid:443 HTTP/1.1")
        );
    }

    /// `.invalid` never resolves here (RFC 2606), so the answer can only have come through the
    /// proxy, which is handed the name to resolve.
    #[test]
    #[ignore = "daily"]
    fn a_request_goes_through_a_socks_proxy() {
        for scheme in ["socks5", "socks5h"] {
            let (port, server) = socks5_proxy();
            let proxy = parse_proxy(&format!("{scheme}://127.0.0.1:{port}")).unwrap();
            let agent = Client::builder()
                .no_proxy()
                .connect_timeout(TIMEOUT)
                .timeout(TIMEOUT)
                .proxy(proxy)
                .build()
                .unwrap();
            let answer = agent.get("http://sunrise.invalid/a.jar").send().unwrap();
            assert_eq!(answer.text().unwrap(), "ok", "{scheme}");
            assert_eq!(server.join().unwrap(), "sunrise.invalid", "{scheme}");
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_stalled_socks_handshake_times_out_and_closes_the_connection() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("socks5h://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            // Never answer the greeting. The client must cancel the handshake and close its socket.
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        });
        let client = Client::builder()
            .no_proxy()
            .proxy(parse_proxy(&proxy).unwrap())
            .connect_timeout(Duration::from_millis(500))
            .timeout(TIMEOUT)
            .build()
            .unwrap();
        let error = client
            .get("http://sunrise.invalid/a.jar")
            .send()
            .unwrap_err();
        assert!(error.is_connect() && error.is_timeout(), "{error}");
        // Keep the client alive while checking EOF, so dropping the whole runtime cannot pass it.
        server.join().unwrap();
    }

    #[test]
    #[ignore = "daily"]
    fn redirects_select_the_proxy_for_each_destination() {
        use std::io::Write;
        use std::net::TcpListener;

        const LOCAL_URL: &str = "SUNRISE_PROXY_TEST_LOCAL_URL";
        if let Ok(origin) = std::env::var(LOCAL_URL) {
            let result = get(
                &format!("{origin}/start"),
                &[("Authorization", "Bearer dummy-origin-token")],
                Redirects::Follow,
                Some(TIMEOUT),
            )
            .unwrap();
            assert_eq!(result.text().unwrap(), "direct again");
            let error = get(
                &format!("{origin}/secure"),
                &[],
                Redirects::Follow,
                Some(TIMEOUT),
            )
            .unwrap_err();
            assert_eq!(error, "HTTPS_PROXY: unsupported proxy scheme");
            return;
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let local_url = format!("http://localhost:{}", listener.local_addr().unwrap().port());
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_url = format!("http://user:pass@{}", proxy.local_addr().unwrap());
        let next = format!("{local_url}/finish");
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = proxy.accept().unwrap();
            let request = read_http_head(&stream);
            assert!(request.starts_with("GET http://sunrise.invalid/asset HTTP/1.1\r\n"));
            let auth = request
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("proxy-authorization"))
                .map(|(_, value)| value.trim());
            assert_eq!(auth, Some("Basic dXNlcjpwYXNz"));
            assert!(!request.contains("dummy-origin-token"));
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: {next}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let origin = std::thread::spawn(move || {
            for (path, reply) in [
                (
                    "/start",
                    "HTTP/1.1 302 Found\r\nLocation: http://sunrise.invalid/asset\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ),
                (
                    "/finish",
                    "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\ndirect again",
                ),
                (
                    "/secure",
                    "HTTP/1.1 302 Found\r\nLocation: https://sunrise.invalid/asset\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_head(&stream);
                assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
                let request = request.to_ascii_lowercase();
                assert!(!request.contains("proxy-authorization"));
                assert_eq!(request.contains("dummy-origin-token"), path == "/start");
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });

        // Set the proxy only in a fresh process, never in the test runner's shared environment.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "tests::redirects_select_the_proxy_for_each_destination",
            ])
            .env(LOCAL_URL, local_url)
            .env("HTTP_PROXY", proxy_url)
            .env(
                "HTTPS_PROXY",
                "https://dummy-user:dummy-password@proxy.example",
            )
            .env("NO_PROXY", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        proxy.join().unwrap();
        origin.join().unwrap();
    }

    /// The client a second request gets is the one the first built: one connection pool, not one
    /// handshake per jar.
    #[test]
    fn a_client_is_kept_for_the_next_request() {
        let mut agents = Agents::new();
        let mut built = Vec::new();
        for spec in [
            None,
            Some("http://localhost:7890"),
            None,
            Some("http://localhost:7891"),
            Some("http://localhost:7890"),
        ] {
            keep(
                &mut agents,
                spec.map(|s| ("HTTP_PROXY", s.into())),
                |proxy| {
                    built.push(spec);
                    let builder = Client::builder().no_proxy();
                    match proxy {
                        Some(proxy) => builder.proxy(proxy),
                        None => builder,
                    }
                    .build()
                    .map_err(|e| e.to_string())
                },
            )
            .unwrap();
        }
        assert_eq!(
            built,
            [
                None,
                Some("http://localhost:7890"),
                Some("http://localhost:7891"),
            ]
        );
    }
}
