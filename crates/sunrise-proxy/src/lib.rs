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
use std::time::Duration;

use reqwest::blocking::{Client, ClientBuilder};

type Agents = HashMap<AgentKey, Client>;

/// A fresh agent starts a new connection pool, so reuse agents with the same settings.
static KEPT: LazyLock<Mutex<Agents>> = LazyLock::new(Mutex::default);

#[derive(PartialEq, Eq, Hash)]
struct AgentKey {
    slot: String,
    proxy: Option<String>,
}

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

/// A kept agent for `url`, using the environment unless the destination is reached directly.
/// An invalid selected proxy is an error, not a direct route.
/// Each `slot` names one builder configuration. `configure` only runs when the slot has no agent
/// for the selected proxy, and should not set a proxy itself.
pub fn agent(
    slot: &str,
    url: &str,
    configure: impl FnOnce(ClientBuilder) -> ClientBuilder,
) -> Result<Client, String> {
    let destination = url::Url::parse(url).ok();
    let selected = if destination
        .as_ref()
        .is_some_and(|url| bypasses_proxy(url, &no_proxy()))
    {
        None
    } else {
        let scheme = destination.as_ref().map_or("https", url::Url::scheme);
        env_proxy(scheme, |name| std::env::var_os(name)).map_err(|e| e.to_string())?
    };
    let mut agents = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    keep(&mut agents, slot, selected, |proxy| {
        let builder = configure(
            Client::builder()
                .no_proxy()
                .user_agent("sunrise-tools")
                .referer(false)
                .connect_timeout(Duration::from_secs(30))
                .timeout(None),
        );
        match proxy {
            Some(proxy) => builder.proxy(proxy),
            None => builder,
        }
        .build()
        .map_err(|e| format!("could not initialize HTTP client: {}", e.without_url()))
    })
}

/// An agent built by `build` and kept for the next request with the same `slot` and proxy.
/// Building a fresh agent every time would give each request a new connection pool.
/// Only `None` requests a direct agent; an invalid `spec` returns an error without calling `build`.
fn keep(
    agents: &mut Agents,
    slot: &str,
    selected: Option<(&'static str, String)>,
    build: impl FnOnce(Option<reqwest::Proxy>) -> Result<Client, String>,
) -> Result<Client, String> {
    let (setting, proxy) = selected.map_or(("", None), |(name, spec)| (name, Some(spec)));
    let key = AgentKey {
        slot: slot.into(),
        proxy,
    };
    match agents.entry(key) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.get().clone()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let proxy = entry
                .key()
                .proxy
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
        url::Url::parse(&format!("{scheme}://{address}")).map_err(|_| InvalidProxy::Authority)?;
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
fn bypasses_proxy(url: &url::Url, no_proxy: &str) -> bool {
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
    use std::time::Duration;

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
        bypasses_proxy(&url::Url::parse(url).unwrap(), no_proxy)
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
    fn a_selected_invalid_proxy_never_builds_a_direct_agent() {
        let mut agents = Agents::new();
        // An existing direct agent must not be reused when selection produces an invalid proxy.
        keep(&mut agents, "invalid", None, |_| {
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
            let error = keep(
                &mut agents,
                "invalid",
                Some(("HTTP_PROXY", spec.into())),
                fail,
            )
            .unwrap_err();
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

    #[test]
    #[ignore = "daily"]
    fn a_refused_connect_is_a_transport_error() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut request = BufReader::new(&stream);
            let mut first = String::new();
            request.read_line(&mut first).unwrap();
            let mut line = String::new();
            while request.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            stream
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            first
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
            server.join().unwrap(),
            "CONNECT sunrise.invalid:443 HTTP/1.1\r\n"
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
    fn a_configured_agent_uses_the_env_proxy_but_goes_direct_for_localhost() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        const LOCAL_URL: &str = "SUNRISE_PROXY_TEST_LOCAL_URL";
        if let Ok(local_url) = std::env::var(LOCAL_URL) {
            let get = |url: &str| {
                agent("test-configured", url, |builder| {
                    builder
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(TIMEOUT)
                        .timeout(TIMEOUT)
                })
                .unwrap()
                .get(url)
                .send()
                .unwrap()
            };
            assert_eq!(get("http://sunrise.invalid/a.jar").text().unwrap(), "ok");
            assert_eq!(
                get(&local_url).status(),
                302,
                "the direct agent must not redirect"
            );
            let error = agent("test-configured", "https://sunrise.invalid/", |_| {
                panic!("a rejected setting must stop before building the agent")
            })
            .unwrap_err();
            assert_eq!(error, "HTTPS_PROXY: unsupported proxy scheme");
            return;
        }

        let (port, proxy) = socks5_proxy();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let local_url = format!(
            "http://localhost:{}/",
            listener.local_addr().unwrap().port()
        );
        let origin = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut line = String::new();
            let mut request = BufReader::new(&stream);
            while request.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });

        // Set the proxy only in a fresh process, never in the test runner's shared environment.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "tests::a_configured_agent_uses_the_env_proxy_but_goes_direct_for_localhost",
            ])
            .env(LOCAL_URL, local_url)
            .env("HTTP_PROXY", format!("socks5h://127.0.0.1:{port}"))
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
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(proxy.join().unwrap(), "sunrise.invalid");
        origin.join().unwrap();
    }

    /// The agent a second request gets is the one the first built: one connection pool, not one
    /// handshake per jar.
    #[test]
    fn an_agent_is_kept_for_the_next_request() {
        let mut agents = Agents::new();
        let mut built = Vec::new();
        for (slot, spec) in [
            ("a", None),
            ("a", Some("http://localhost:7890")),
            ("a", None),
            ("a", Some("http://localhost:7891")),
            ("a", Some("http://localhost:7890")),
            ("b", Some("http://localhost:7890")),
        ] {
            keep(
                &mut agents,
                slot,
                spec.map(|s| ("HTTP_PROXY", s.into())),
                |proxy| {
                    built.push((slot, spec));
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
                ("a", None),
                ("a", Some("http://localhost:7890")),
                ("a", Some("http://localhost:7891")),
                ("b", Some("http://localhost:7890")),
            ]
        );
    }
}
