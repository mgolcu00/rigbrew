//! The two tools that leave the machine: a web search and reading one page. They are only
//! offered to the model when the person switched "Allow web search" on.
//!
//! A page is only fetched from a public address: loopback, private, link-local and other
//! special ranges are refused, for the address a name resolves to as well as for a literal
//! one, and again at every redirect. The connection goes to the address that was checked.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::Url;

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
/// Results of one search. Few and short: a small model reads every one of them.
const MAX_RESULTS: usize = 5;
const MAX_TITLE_CHARS: usize = 100;
const MAX_SNIPPET_CHARS: usize = 160;
const USER_AGENT: &str = concat!("Mozilla/5.0 (compatible; Rigbrew/", env!("CARGO_PKG_VERSION"), ")");

fn public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        // Carrier-grade NAT, protocol assignments, benchmarking, reserved.
        || (o[0] == 100 && (o[1] & 0xc0) == 64)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (o[1] & 0xfe) == 18)
        || o[0] >= 240)
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    // NAT64 carries an IPv4 address in its last 32 bits.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        return public_v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique local fc00::/7, link-local fe80::/10, site-local fec0::/10, documentation 2001:db8::/32.
        || (s[0] & 0xfe00) == 0xfc00
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        // IPv4-compatible ::a.b.c.d
        || s[..6].iter().all(|x| *x == 0))
}

/// Whether an address is one on the public internet.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

/// Checks what can be checked without the network: http or https, a host, no sign-in part, and
/// no address that is plainly not public.
pub fn check_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| format!("{raw} is not a web address."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http and https pages can be fetched.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Addresses with a user name or password in them are not fetched.".into());
    }
    let refused = || "That address is on this computer or a private network. Only public pages can be fetched.".to_string();
    match url.host() {
        None => Err(format!("{raw} has no host.")),
        Some(url::Host::Ipv4(ip)) if !public_v4(ip) => Err(refused()),
        Some(url::Host::Ipv6(ip)) if !public_v6(ip) => Err(refused()),
        Some(url::Host::Domain(name)) if name.eq_ignore_ascii_case("localhost") || name.to_ascii_lowercase().ends_with(".localhost") => {
            Err(refused())
        }
        Some(_) => Ok(url),
    }
}

/// An address on this computer, the only kind `look_at_page` opens: http or https, and
/// `localhost`, a name under it, or a loopback address. "0.0.0.0", which servers print for
/// "every address", is read as this computer.
pub fn local_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    let mut url = Url::parse(raw)
        .map_err(|_| format!("{raw} is not a web address. Use the address of the running app, like http://localhost:5173/."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http pages on this computer can be looked at, like http://localhost:5173/.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Addresses with a user name or password in them are not opened.".into());
    }
    if matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_unspecified()) {
        url.set_host(Some("localhost")).map_err(|_| "That address cannot be opened.".to_string())?;
    }
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost") || name.to_ascii_lowercase().ends_with(".localhost"),
        None => false,
    };
    if !local {
        return Err("look_at_page only opens pages on this computer, like http://localhost:5173/. A page on the internet is read with fetch_url, when the person allowed the web.".into());
    }
    Ok(url)
}

/// The addresses of the URL's host, all of them public, or an error.
async fn public_addrs(url: &Url) -> Result<Vec<SocketAddr>, String> {
    let host = url.host_str().ok_or("The address has no host.")?;
    let port = url.port_or_known_default().unwrap_or(443);
    let found: Vec<SocketAddr> = tokio::net::lookup_host((host.trim_start_matches('[').trim_end_matches(']'), port))
        .await
        .map_err(|_| format!("{host} could not be found."))?
        .collect();
    if found.is_empty() {
        return Err(format!("{host} could not be found."));
    }
    if found.iter().any(|a| !public_ip(a.ip())) {
        return Err("That address is on this computer or a private network. Only public pages can be fetched.".into());
    }
    Ok(found)
}

struct Page {
    url: Url,
    content_type: String,
    body: Vec<u8>,
    cut: bool,
}

async fn get(start: Url) -> Result<Page, String> {
    use futures_util::StreamExt;
    let mut url = start;
    for _ in 0..=MAX_REDIRECTS {
        let url_checked = check_url(url.as_str())?;
        let addrs = public_addrs(&url_checked).await?;
        // The connection is pinned to the addresses that were just checked, so the name cannot
        // resolve to something else between the check and the request.
        let mut builder =
            reqwest::Client::builder().user_agent(USER_AGENT).redirect(reqwest::redirect::Policy::none()).no_proxy().timeout(TIMEOUT);
        if let Some(url::Host::Domain(name)) = url_checked.host() {
            builder = builder.resolve_to_addrs(name, &addrs);
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        let resp = client
            .get(url_checked.clone())
            .header("Accept", "text/html,text/plain,application/json;q=0.9,*/*;q=0.5")
            .header("Accept-Language", "en")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "The page took too long to answer.".to_string()
                } else {
                    format!("The page could not be reached: {}", short(&e.to_string()))
                }
            })?;
        if resp.status().is_redirection() {
            let to = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("The page redirects without saying where to.")?;
            url = url_checked.join(to).map_err(|_| "The page redirects to something that is not a web address.".to_string())?;
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!("The page answered with HTTP {}.", resp.status().as_u16()));
        }
        let content_type =
            resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let mut body = Vec::new();
        let mut cut = false;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| if e.is_timeout() { "The page took too long to answer.".to_string() } else { short(&e.to_string()) })?;
            let room = MAX_BYTES - body.len();
            if chunk.len() >= room {
                body.extend_from_slice(&chunk[..room]);
                cut = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        return Ok(Page { url: url_checked, content_type, body, cut });
    }
    Err("The page redirects too many times.".into())
}

fn short(text: &str) -> String {
    text.chars().take(160).collect()
}

/// The text, or its first `max` characters and "…".
fn short_to(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max - 1).collect::<String>().trim_end())
}

/// Named and numeric character references, the few that matter for reading.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let re = regex::Regex::new(r"&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z]{2,8});").expect("static pattern");
    re.replace_all(text, |c: &regex::Captures| {
        let name = &c[1];
        let ch = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = name.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "rsquo" | "lsquo" => Some('\''),
                "rdquo" | "ldquo" => Some('"'),
                "copy" => Some('©'),
                _ => None,
            }
        };
        ch.map(String::from).unwrap_or_else(|| c[0].to_string())
    })
    .into_owned()
}

fn collapse(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(&line);
    }
    out
}

/// The text of an HTML page: no scripts, styles or tags, one blank line between blocks.
pub fn readable(html: &str) -> String {
    let re = |p: &str| regex::Regex::new(p).expect("static pattern");
    // The article itself when the page marks it; menus and page furniture never.
    let body = re(r"(?is)<main\b.*</main\s*>")
        .find(html)
        .or_else(|| re(r"(?is)<article\b.*</article\s*>").find(html))
        .map_or(html, |m| m.as_str());
    let mut text = body.to_string();
    for tag in ["script", "style", "noscript", "svg", "head", "template", "iframe", "nav", "footer", "aside", "form", "button", "select"] {
        text = re(&format!(r"(?is)<{tag}\b.*?</{tag}\s*>")).replace_all(&text, " ").into_owned();
    }
    let text = re(r"(?s)<!--.*?-->").replace_all(&text, " ");
    let text = re(r"(?i)<br\s*/?>").replace_all(&text, "\n");
    let text =
        re(r"(?i)</?(p|div|section|article|header|footer|main|nav|aside|h[1-6]|ul|ol|table|tr|pre|blockquote|form|dl|dt|dd)\b[^>]*>")
            .replace_all(&text, "\n\n");
    let text = re(r"(?i)<li\b[^>]*>").replace_all(&text, "\n- ");
    let text = re(r"(?s)<[^>]*>").replace_all(&text, "");
    collapse(&decode_entities(&text))
}

/// `fetch_url`: the readable text of one public page.
pub async fn fetch_url(raw: &str) -> Result<String, String> {
    let url = check_url(raw)?;
    let page = tokio::time::timeout(TIMEOUT, get(url)).await.map_err(|_| "The page took too long to answer.".to_string())??;
    let kind = page.content_type.split(';').next().unwrap_or("").trim().to_string();
    let texty = kind.is_empty() || kind.starts_with("text/") || ["json", "xml", "javascript"].iter().any(|k| kind.contains(k));
    if !texty || page.body.iter().take(4096).any(|b| *b == 0) {
        return Err(format!("{} is not a text page ({kind}).", page.url));
    }
    let raw_text = String::from_utf8_lossy(&page.body);
    let text = if kind.contains("html") || raw_text.trim_start().starts_with("<!") || raw_text.trim_start().starts_with("<html") {
        readable(&raw_text)
    } else {
        raw_text.trim().to_string()
    };
    if text.is_empty() {
        return Err("The page has no readable text. It may need a browser to show anything.".into());
    }
    let note = if page.cut { "\n[truncated: the page is larger than 2 MB; only the first part was read]" } else { "" };
    Ok(format!("{}\n\n{}{note}", page.url, super::files::cap(&text, super::files::RESULT_CHARS)))
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// The results in the answer of Rigbrew's server (`GET /v1/search`): few, short, public addresses only.
pub fn parse_results(answer: &serde_json::Value) -> Vec<Hit> {
    let Some(list) = answer["results"].as_array() else { return vec![] };
    list.iter()
        .filter_map(|r| {
            let url = check_url(r["url"].as_str()?).ok()?.to_string();
            let title = collapse(r["title"].as_str().unwrap_or(""));
            if title.is_empty() {
                return None;
            }
            Some(Hit {
                title: short_to(&title, MAX_TITLE_CHARS),
                url,
                snippet: short_to(&collapse(r["snippet"].as_str().unwrap_or("")), MAX_SNIPPET_CHARS),
            })
        })
        .take(MAX_RESULTS)
        .collect()
}

/// What the model is told when the search could not be done: searching again will not help,
/// so this is an error, and after two of them in a row the search is switched off for the
/// request (see `pace`).
pub const NO_ANSWER: &str = "The web search is not available right now. Searching again will not help now. \
Open a page you know with fetch_url, such as the official site or its documentation, or answer from what you know and say that it is not checked.";

/// The results as the model reads them: a short numbered list, or that there are none.
pub fn results_text(answer: &serde_json::Value) -> String {
    let hits = parse_results(answer);
    if hits.is_empty() {
        return "The search found nothing for these words. Use fewer or other words, once.".into();
    }
    let lines: Vec<String> =
        hits.iter().enumerate().map(|(i, h)| format!("{}. {}\n   {}\n   {}", i + 1, h.title, h.url, h.snippet)).collect();
    super::files::cap(&lines.join("\n"), super::files::RESULT_CHARS)
}

/// `web_search`: the first results for a query, as title, address and a line of text each.
/// The words go to Rigbrew's server, which asks a search service and keeps nothing.
pub async fn search(server: &str, net: &reqwest::Client, query: &str) -> Result<String, String> {
    let query: String = query.trim().chars().take(300).collect();
    if query.is_empty() {
        return Err("A search needs some words to look for.".into());
    }
    let mut url = Url::parse(&format!("{server}/v1/search")).map_err(|_| NO_ANSWER.to_string())?;
    url.query_pairs_mut().append_pair("q", &query);
    let resp = net.get(url).timeout(TIMEOUT).send().await.map_err(|e| {
        if e.is_timeout() {
            "The search took too long to answer.".to_string()
        } else {
            NO_ANSWER.to_string()
        }
    })?;
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("The search is busy. Work with what you have, or open a page you know with fetch_url.".into());
    }
    if !resp.status().is_success() {
        return Err(NO_ANSWER.into());
    }
    let answer: serde_json::Value = resp.json().await.map_err(|_| NO_ANSWER.to_string())?;
    Ok(results_text(&answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_pass() {
        for ok in ["93.184.216.34", "8.8.8.8", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(public_ip(ok.parse().unwrap()), "{ok}");
        }
        for no in [
            "127.0.0.1",
            "127.8.9.1",
            "10.0.0.5",
            "172.16.3.4",
            "192.168.1.115",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
            "64:ff9b::7f00:1",
            "::127.0.0.1",
        ] {
            assert!(!public_ip(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn urls_are_checked_before_anything_is_sent() {
        assert!(check_url("https://example.com/a?b=1").is_ok());
        assert!(check_url("http://example.com:8080/").is_ok());
        for no in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "javascript:alert(1)",
            "example.com",
            "http://localhost:8080/",
            "http://app.localhost/",
            "http://127.0.0.1:11434/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://[::1]/",
            "http://[::ffff:10.0.0.1]/",
            "http://192.168.1.1/admin",
            "http://169.254.169.254/latest/meta-data/",
            "https://user:pass@example.com/",
            "http://",
        ] {
            assert!(check_url(no).is_err(), "{no}");
        }
    }

    #[tokio::test]
    async fn names_that_resolve_to_this_machine_are_refused() {
        let err = fetch_url("http://localhost:1/").await.unwrap_err();
        assert!(err.contains("private network"), "{err}");
        let err = fetch_url("http://127.0.0.1:1/").await.unwrap_err();
        assert!(err.contains("private network"), "{err}");
    }

    #[test]
    fn a_look_only_opens_this_computer() {
        for ok in [
            "http://localhost:5173/",
            "http://localhost:5173/cart?x=1#top",
            " http://127.0.0.1:8000 ",
            "http://127.8.9.1:3000/",
            "http://[::1]:3000/",
            "https://localhost:8443/",
            "http://shop.localhost:5173/",
            "http://LOCALHOST:5173/",
        ] {
            assert!(local_url(ok).is_ok(), "{ok}");
        }
        // What a server prints for "every address" is this computer.
        assert_eq!(local_url("http://0.0.0.0:8000/a").unwrap().as_str(), "http://localhost:8000/a");
        for refused in [
            "https://example.com/",
            "http://localhost.example.com/",
            "http://example.com@localhost:5173/",
            "http://user:pw@localhost:5173/",
            "http://192.168.1.20:5173/",
            "http://10.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::ffff:8.8.8.8]/",
            "http://2130706433.example/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "localhost:5173",
            "",
        ] {
            assert!(local_url(refused).is_err(), "{refused}");
        }
        assert!(local_url("https://example.com/").unwrap_err().contains("only opens pages on this computer"));
    }

    #[test]
    fn pages_become_plain_text() {
        let html = r#"<!doctype html><html><head><title>T</title><style>p{color:red}</style></head>
            <body><script>var x = "<p>not this</p>";</script><h1>Head&amp;line</h1>
            <p>First   paragraph<br>second line.</p><!-- hidden --><ul><li>one</li><li>two &#8211; 2</li></ul>
            <noscript>enable js</noscript><p>Caf&#xe9; &copy; &unknown;</p></body></html>"#;
        assert_eq!(readable(html), "Head&line\n\nFirst paragraph\nsecond line.\n\n- one\n- two – 2\n\nCafé © &unknown;");
        let page = "<body><nav><a href=/>Home</a></nav><div>Cookie banner</div><main><nav>On this page</nav><h1>Title</h1><p>Text.</p>\
            <form><button>Send</button></form></main><footer>About us</footer></body>";
        assert_eq!(readable(page), "Title\n\nText.");
        assert_eq!(readable("<div>before</div><article><p>Story</p></article><aside>Ads</aside>"), "Story");
    }

    #[test]
    fn search_results_are_read_from_the_answer() {
        let answer = serde_json::json!({"results": [
            {"title": "Learn  Rust", "url": "https://www.rust-lang.org/learn", "snippet": "Get started with Rust & friends."},
            {"title": "The Book", "url": "https://doc.rust-lang.org/book/"},
            {"title": "Local", "url": "http://127.0.0.1/admin", "snippet": "x"},
            {"title": "", "url": "https://example.com/"},
            {"title": "Odd", "url": "file:///etc/passwd"},
        ]});
        assert_eq!(
            parse_results(&answer),
            vec![
                Hit {
                    title: "Learn Rust".into(),
                    url: "https://www.rust-lang.org/learn".into(),
                    snippet: "Get started with Rust & friends.".into()
                },
                Hit { title: "The Book".into(), url: "https://doc.rust-lang.org/book/".into(), snippet: String::new() },
            ]
        );
        assert!(parse_results(&serde_json::json!({"error": {"code": "search_off"}})).is_empty());
        assert!(results_text(&serde_json::json!({"results": []})).starts_with("The search found nothing"));
    }

    #[test]
    fn results_are_few_and_short() {
        let long = "word ".repeat(100);
        let list: Vec<_> = (0..20)
            .map(|i| serde_json::json!({"title": format!("Result {i} {long}"), "url": format!("https://example.com/{i}"), "snippet": format!("Text {i} {long}")}))
            .collect();
        let answer = serde_json::json!({"results": list});
        let hits = parse_results(&answer);
        assert_eq!(hits.len(), 5);
        assert!(hits[4].snippet.starts_with("Text 4 word") && hits[4].snippet.ends_with('…'));
        assert!(hits.iter().all(|h| h.snippet.chars().count() <= 160 && h.title.chars().count() <= 100));
        // Five results as the model reads them fit in a small part of its context.
        let shown = results_text(&answer);
        assert!(shown.starts_with("1. Result 0 word") && shown.contains("\n5. Result 4") && !shown.contains("\n6. "), "{shown}");
        assert!(shown.chars().count() < 1600, "{}", shown.chars().count());
    }
}
