//! Port-pack's download agents and redirect checks.

/// An agent for downloading `url`: through the proxy when one is set, unless `url` is a host that
/// must be reached directly.
pub(crate) fn agent(url: &str) -> Result<reqwest::blocking::Client, String> {
    sunrise_proxy::agent("port-pack", url, std::convert::identity)
}

/// GET from allowed HTTPS hosts, validating every redirect before contacting it.
pub(crate) fn get_from_hosts(
    url: &str,
    hosts: &[&str],
) -> Result<Option<reqwest::blocking::Response>, String> {
    follow(url, hosts, |address| {
        let agent = sunrise_proxy::agent("port-pack-no-redirect", address, |builder| {
            builder.redirect(reqwest::redirect::Policy::none())
        })
        .map_err(|e| format!("{address}: {e}"))?;
        let response = agent
            .get(address)
            .header("User-Agent", crate::modrinth::AGENT)
            .send()
            .map_err(|e| format!("{address}: {e}"))?;
        if response.status() == 404 {
            return Ok(None);
        }
        response
            .error_for_status()
            .map(Some)
            .map_err(|e| format!("{address}: {e}"))
    })
}

fn follow(
    url: &str,
    hosts: &[&str],
    mut request: impl FnMut(&str) -> Result<Option<reqwest::blocking::Response>, String>,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let mut current = checked_url(url, hosts)?;
    for _ in 0..=5 {
        let Some(response) = request(current.as_str())? else {
            return Ok(None);
        };
        if !response.status().is_redirection() {
            return Ok(Some(response));
        }
        let location = response
            .headers()
            .get("Location")
            .ok_or("Redirect without Location")?
            .to_str()
            .map_err(|_| "Invalid redirect Location")?;
        current = checked_url(
            current.join(location).map_err(|e| e.to_string())?.as_str(),
            hosts,
        )?;
    }
    Err("Too many redirects".into())
}

fn checked_url(address: &str, hosts: &[&str]) -> Result<url::Url, String> {
    let url = url::Url::parse(address).map_err(|e| e.to_string())?;
    if url.scheme() != "https"
        || !url.host_str().is_some_and(|host| hosts.contains(&host))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Refused download URL outside allowed HTTPS hosts".into());
    }
    Ok(url)
}

#[cfg(test)]
mod redirect_tests {
    use super::*;
    fn response(location: Option<&str>) -> reqwest::blocking::Response {
        let builder = http::Response::builder();
        let builder = match location {
            Some(location) => builder.status(302).header("Location", location),
            None => builder.status(200),
        };
        builder.body("").unwrap().into()
    }

    #[test]
    fn checks_each_redirect_before_requesting_it() {
        let mut seen = Vec::new();
        let result = follow("https://cdn.modrinth.com/a", &["cdn.modrinth.com"], |url| {
            seen.push(url.to_owned());
            let location = if seen.len() == 1 {
                "/b"
            } else {
                "https://evil.example/steal"
            };
            Ok(Some(response(Some(location))))
        });
        assert!(result.is_err());
        assert_eq!(
            seen,
            ["https://cdn.modrinth.com/a", "https://cdn.modrinth.com/b"]
        );
        let mut seen = Vec::new();
        let result = follow(
            "https://github.com/a",
            &["github.com", "release-assets.githubusercontent.com"],
            |url| {
                seen.push(url.to_owned());
                Ok(Some(response((seen.len() == 1).then_some(
                    "https://release-assets.githubusercontent.com/file",
                ))))
            },
        )
        .unwrap();
        assert!(result.is_some());
        assert_eq!(seen.len(), 2);
        for url in [
            "http://cdn.modrinth.com/a",
            "https://cdn.modrinth.com.evil/a",
            "https://user@cdn.modrinth.com/a",
            "https://cdn.modrinth.com:444/a",
        ] {
            assert!(checked_url(url, &["cdn.modrinth.com"]).is_err());
        }
        let mut count = 0;
        assert!(
            follow("https://cdn.modrinth.com/a", &["cdn.modrinth.com"], |_| {
                count += 1;
                Ok(Some(response(Some("/a"))))
            })
            .is_err()
        );
        assert_eq!(count, 6);
        assert!(
            follow("https://cdn.modrinth.com/a", &["cdn.modrinth.com"], |_| Ok(
                None
            ))
            .unwrap()
            .is_none()
        );
        assert!(
            follow(
                "https://cdn.modrinth.com/a",
                &["cdn.modrinth.com"],
                |_| Err("network failure".into())
            )
            .is_err()
        );
    }
}
