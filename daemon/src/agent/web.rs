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
const MAX_RESULTS: usize = 8;
const SEARCH: &str = "https://html.duckduckgo.com/html/";
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

#[derive(Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn strip_tags(html: &str) -> String {
    let no_tags = regex::Regex::new(r"(?s)<[^>]*>").expect("static pattern").replace_all(html, "");
    decode_entities(&no_tags).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The address a result link really leads to: DuckDuckGo wraps it as `/l/?uddg=<address>`.
fn unwrap_link(href: &str) -> Option<String> {
    let href = decode_entities(href);
    let full = if href.starts_with("//") { format!("https:{href}") } else { href };
    let url = Url::parse(&full).or_else(|_| Url::parse(SEARCH).and_then(|b| b.join(&full))).ok()?;
    let wrapped = url.host_str().is_some_and(|h| h.ends_with("duckduckgo.com"));
    if wrapped {
        // Sponsored links go through y.js and carry no plain address.
        if url.path() != "/l/" {
            return None;
        }
        let target = url.query_pairs().find(|(k, _)| k == "uddg").map(|(_, v)| v.into_owned())?;
        return check_url(&target).ok().map(|u| u.to_string());
    }
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// Titles, addresses and snippets out of DuckDuckGo's HTML results page.
pub fn parse_results(html: &str) -> Vec<Hit> {
    let title = regex::Regex::new(r#"(?s)<a\b[^>]*class="[^"]*\bresult__a\b[^"]*"[^>]*>(.*?)</a>"#).expect("static pattern");
    let href = regex::Regex::new(r#"href="([^"]+)""#).expect("static pattern");
    let snippet = regex::Regex::new(r#"(?s)<a\b[^>]*class="[^"]*\bresult__snippet\b[^"]*"[^>]*>(.*?)</a>"#).expect("static pattern");
    let titles: Vec<_> = title.captures_iter(html).collect();
    let mut out = Vec::new();
    for (i, t) in titles.iter().enumerate() {
        let whole = t.get(0).expect("match");
        let Some(url) = href.captures(whole.as_str()).and_then(|h| unwrap_link(&h[1])) else { continue };
        // The snippet of a result sits between its title and the next title.
        let until = titles.get(i + 1).map_or(html.len(), |n| n.get(0).expect("match").start());
        let text = snippet.captures(&html[whole.end()..until]).map(|s| strip_tags(&s[1])).unwrap_or_default();
        let name = strip_tags(&t[1]);
        if name.is_empty() {
            continue;
        }
        out.push(Hit { title: name, url, snippet: text });
        if out.len() == MAX_RESULTS {
            break;
        }
    }
    out
}

/// `web_search`: the first results for a query, as title, address and a line of text each.
pub async fn search(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("A search needs some words to look for.".into());
    }
    let mut url = Url::parse(SEARCH).expect("static address");
    url.query_pairs_mut().append_pair("q", &query.chars().take(300).collect::<String>());
    let page = tokio::time::timeout(TIMEOUT, get(url)).await.map_err(|_| "The search took too long to answer.".to_string())??;
    let hits = parse_results(&String::from_utf8_lossy(&page.body));
    if hits.is_empty() {
        return Ok("The search gave no results that could be read. Try other words.".into());
    }
    let lines: Vec<String> =
        hits.iter().enumerate().map(|(i, h)| format!("{}. {}\n   {}\n   {}", i + 1, h.title, h.url, h.snippet)).collect();
    Ok(super::files::cap(&lines.join("\n"), super::files::RESULT_CHARS))
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
    fn search_results_are_read_from_the_page() {
        let html = r#"
        <div class="result results_links results_links_deep web-result ">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2Flearn&amp;rut=abc">Learn <b>Rust</b></a></h2>
          <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2Flearn&amp;rut=abc">Get started with <b>Rust</b> &amp; friends.</a>
        </div>
        <div class="result result--ad">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=example.com&amp;u3=x">An ad</a></h2>
          <a class="result__snippet" href="https://duckduckgo.com/y.js?x">Buy things.</a>
        </div>
        <div class="result">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="https://doc.rust-lang.org/book/">The Book</a></h2>
        </div>
        <div class="result">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=http%3A%2F%2F127.0.0.1%2Fadmin">Local</a></h2>
        </div>"#;
        let hits = parse_results(html);
        assert_eq!(
            hits,
            vec![
                Hit {
                    title: "Learn Rust".into(),
                    url: "https://www.rust-lang.org/learn".into(),
                    snippet: "Get started with Rust & friends.".into()
                },
                Hit { title: "The Book".into(), url: "https://doc.rust-lang.org/book/".into(), snippet: String::new() },
            ]
        );
        assert!(parse_results("<html><body>Nothing here</body></html>").is_empty());
    }

    #[test]
    fn at_most_eight_results() {
        let one = |i: usize| {
            format!(r#"<a class="result__a" href="https://example.com/{i}">Result {i}</a><a class="result__snippet">Text {i}</a>"#)
        };
        let html: String = (0..20).map(one).collect();
        let hits = parse_results(&html);
        assert_eq!(hits.len(), 8);
        assert_eq!(hits[7].snippet, "Text 7");
    }
}
