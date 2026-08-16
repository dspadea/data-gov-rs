//! Checks that stand between harvested metadata and the machine.
//!
//! Two kinds of value arrive in third-party DCAT records and are treated as
//! hostile: the text a filename is derived from, and the URL a download is
//! fetched from. This module holds the guards for both - path-component
//! sanitizing on one side, and the download destination rules on the other.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::{Host, Url};

use crate::error::{DataGovError, Result};

/// The most redirect hops a download may take before it is abandoned.
///
/// The client follows redirects itself rather than delegating to reqwest, so
/// no chain limit is inherited from anywhere and the cap is enforced by
/// [`fetch_checked`].
pub(crate) const MAX_REDIRECT_HOPS: usize = 10;

/// The response statuses that name somewhere else to go.
///
/// The same set reqwest follows. 301, 302 and 303 may change the method for a
/// request that was not a GET; 307 and 308 never do.
const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];

/// An address range a download is not allowed to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockedRange {
    /// `127.0.0.0/8` and `::1`.
    Loopback,
    /// `169.254.0.0/16` and `fe80::/10`.
    LinkLocal,
    /// The RFC 1918 ranges.
    Private,
    /// `fc00::/7`.
    UniqueLocal,
    /// `100.64.0.0/10`.
    CarrierGradeNat,
    /// `0.0.0.0` and `::`, which route to the local host.
    Unspecified,
}

impl BlockedRange {
    /// A short name for the range, for the refusal message.
    fn describe(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::LinkLocal => "link-local",
            Self::Private => "private",
            Self::UniqueLocal => "unique-local",
            Self::CarrierGradeNat => "carrier-grade-NAT",
            Self::Unspecified => "unspecified",
        }
    }

    /// Whether the private-network opt-in reaches this range.
    ///
    /// It reaches every range but link-local. `169.254.0.0/16` and `fe80::/10`
    /// carry the cloud instance-metadata services, which is the destination
    /// this check exists for, and no mirror is served from there.
    fn opt_in_applies(self) -> bool {
        !matches!(self, Self::LinkLocal)
    }
}

/// A download destination the client refused to reach.
///
/// It travels as the cause of a [`reqwest::Error`], so a refusal decided inside
/// the DNS resolver - which cannot return a [`DataGovError`] - is recoverable
/// by [`refusal_in`] and can be reported as a validation failure rather than as
/// a transport failure.
#[derive(Debug)]
pub(crate) struct RefusedDestination(String);

impl RefusedDestination {
    /// Wrap a refusal message.
    pub(crate) fn new<S: Into<String>>(message: S) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RefusedDestination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for RefusedDestination {}

/// A DNS resolver that will not hand back an address a download may not reach.
///
/// [`fetch_checked`] judges the host of every URL a download requests, redirect
/// hops included. This is the layer below it: reqwest calls this resolver when
/// it opens a connection, and connects only to the addresses it returns, so the
/// addresses actually connected to are checked and not only the ones the URL
/// check saw.
///
/// # A limit worth stating
///
/// The two layers exist because a name is resolved twice - once to judge it,
/// once to connect - and DNS is under the control of whoever published the
/// record. Checking the addresses reqwest connects to narrows that window; it
/// does not close it. Two cases stay outside what a client at this layer can
/// decide: an answer that changes between the two lookups, and a connection
/// made through an HTTP proxy, where reqwest resolves the proxy's host and this
/// resolver never sees the destination's at all.
#[derive(Debug)]
pub(crate) struct GuardedResolver {
    allow_private: bool,
}

impl GuardedResolver {
    /// Build a resolver, optionally permitting private-network destinations.
    pub(crate) fn new(allow_private: bool) -> Self {
        Self { allow_private }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let allow_private = self.allow_private;
        let host = name.as_str().to_string();
        Box::pin(async move {
            // Port 0: reqwest substitutes the port from the URL, or the
            // conventional port for the scheme.
            let resolved: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            for address in &resolved {
                if let Some(message) = address_refusal(&host, address.ip(), allow_private) {
                    return Err(Box::new(RefusedDestination::new(message))
                        as Box<dyn StdError + Send + Sync>);
                }
            }
            Ok(Box::new(resolved.into_iter()) as Addrs)
        })
    }
}

/// Name the range an address belongs to, when downloads may not reach it.
pub(crate) fn classify_address(ip: IpAddr) -> Option<BlockedRange> {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_unspecified() {
                return Some(BlockedRange::Unspecified);
            }
            if v4.is_loopback() {
                return Some(BlockedRange::Loopback);
            }
            if v4.is_link_local() {
                return Some(BlockedRange::LinkLocal);
            }
            if v4.is_private() {
                return Some(BlockedRange::Private);
            }
            let [first, second, ..] = v4.octets();
            if first == 100 && (64..=127).contains(&second) {
                return Some(BlockedRange::CarrierGradeNat);
            }
            None
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                return Some(BlockedRange::Unspecified);
            }
            if v6.is_loopback() {
                return Some(BlockedRange::Loopback);
            }
            // An IPv4 address in an IPv6 costume routes as IPv4 and gets the
            // IPv4 rules. Without this, `::ffff:127.0.0.1` walks straight past.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return classify_address(IpAddr::V4(v4));
            }
            let segments = v6.segments();
            if segments[0] & 0xffc0 == 0xfe80 {
                return Some(BlockedRange::LinkLocal);
            }
            if segments[0] & 0xfe00 == 0xfc00 {
                return Some(BlockedRange::UniqueLocal);
            }
            // `::a.b.c.d`, the deprecated IPv4-compatible form, routes as IPv4
            // too. `::` and `::1` were already answered above.
            if segments[..6] == [0, 0, 0, 0, 0, 0] {
                let octets = v6.octets();
                let v4 = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                return classify_address(IpAddr::V4(v4));
            }
            None
        }
    }
}

/// Say why `ip` is refused for `host`, or `None` when it is permitted.
pub(crate) fn address_refusal(host: &str, ip: IpAddr, allow_private: bool) -> Option<String> {
    let range = classify_address(ip)?;
    if allow_private && range.opt_in_applies() {
        return None;
    }
    Some(format!(
        "download destination `{host}` resolves to {ip}, a {} address, which downloads may not reach",
        range.describe()
    ))
}

/// Judge a download URL on the evidence available without a DNS lookup.
///
/// Returns the host name when the URL names a host that still has to be
/// resolved, and `None` when the host was a literal address this already
/// judged. The error is the refusal message.
///
/// This is the half of the check that a synchronous
/// [`reqwest::redirect::Policy`] callback can run.
pub(crate) fn check_url_without_dns(
    url: &Url,
    allow_private: bool,
) -> std::result::Result<Option<String>, String> {
    let scheme = url.scheme();
    if !matches!(scheme, "http" | "https") {
        return Err(format!(
            "download URL scheme `{scheme}` is not supported, downloads use http or https"
        ));
    }
    match url.host() {
        None => Err(format!("download URL `{url}` names no host")),
        Some(Host::Domain(name)) => Ok(Some(name.to_string())),
        Some(Host::Ipv4(v4)) => {
            match address_refusal(&v4.to_string(), IpAddr::V4(v4), allow_private) {
                Some(message) => Err(message),
                None => Ok(None),
            }
        }
        Some(Host::Ipv6(v6)) => {
            match address_refusal(&v6.to_string(), IpAddr::V6(v6), allow_private) {
                Some(message) => Err(message),
                None => Ok(None),
            }
        }
    }
}

/// Check a download URL before the request leaves.
///
/// `lookup_timeout` bounds the name lookup; see [`check_download_url_with`].
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when the URL does not parse, uses
/// a scheme other than `http` or `https`, names no host, is not resolved
/// within `lookup_timeout`, cannot be resolved at all, or points at an address
/// downloads may not reach.
///
/// # A limit worth stating
///
/// A host given as a name is resolved here and judged on the answer. Between
/// that answer and the connection reqwest opens, the name can be answered
/// differently - the DNS-rebinding case. [`GuardedResolver`] narrows that
/// window by checking the addresses reqwest actually connects to, but a client
/// at this layer cannot close it.
pub(crate) async fn check_download_url(
    raw: &str,
    allow_private: bool,
    lookup_timeout: Duration,
) -> Result<()> {
    check_download_url_with(raw, allow_private, lookup_timeout, |host, port| {
        tokio::net::lookup_host((host, port))
    })
    .await
}

/// The body of [`check_download_url`], with the name lookup supplied.
///
/// Taking the lookup as an argument is what makes the bound testable: a test
/// drives a lookup that answers late, or not at all, without a name server and
/// without depending on how long anything really takes.
///
/// # Why the lookup needs a bound of its own
///
/// This runs outside reqwest, so neither of the download client's bounds
/// reaches it. `read_timeout` is a property of the response body, so it starts
/// only once a request is in flight, and `connect_timeout` wraps reqwest's own
/// connector, which covers the lookup [`GuardedResolver`] does and not this
/// one. A name server that accepts a query and never answers therefore had
/// nothing to end it. Cutting it off does not contradict the rule that work
/// which is progressing is never killed on elapsed time: a lookup that is not
/// answering is not progressing, and no byte of any transfer is in flight yet.
///
/// # Why the bound is passed rather than derived
///
/// The value is the download's own stall bound, `download_timeout_secs`, which
/// already bounds the connect and each read of the body. One setting has to
/// govern all three. Deriving a constant here would give an operator who
/// raises the setting for a slow or distant link two thirds of what they asked
/// for, and a third bound they can neither see nor change.
///
/// # A limit worth stating
///
/// The bound frees the caller, not the thread. `tokio::net::lookup_host` runs
/// `getaddrinfo` on the blocking pool, and that call cannot be cancelled
/// portably, so the pool task runs on until the system resolver gives up. What
/// this ends is the download - and, above it, the MCP request - waiting on it.
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when the URL does not parse, uses
/// a scheme other than `http` or `https`, names no host, is not resolved
/// within `lookup_timeout`, cannot be resolved at all, or points at an address
/// downloads may not reach. A lookup that ran out of time is reported
/// separately from one that answered "no such host", so an operator can tell a
/// silent name server from a name that does not exist.
async fn check_download_url_with<L, F, I>(
    raw: &str,
    allow_private: bool,
    lookup_timeout: Duration,
    lookup: L,
) -> Result<()>
where
    L: FnOnce(String, u16) -> F,
    F: Future<Output = std::io::Result<I>>,
    I: Iterator<Item = SocketAddr>,
{
    let url = Url::parse(raw).map_err(|err| {
        DataGovError::validation_error(format!("download URL `{raw}` does not parse: {err}"))
    })?;

    let Some(host) =
        check_url_without_dns(&url, allow_private).map_err(DataGovError::validation_error)?
    else {
        return Ok(());
    };

    let port = url.port_or_known_default().unwrap_or(80);
    let resolved = match tokio::time::timeout(lookup_timeout, lookup(host.clone(), port)).await {
        Err(_elapsed) => {
            return Err(DataGovError::validation_error(format!(
                "download URL host `{host}` did not resolve within {lookup_timeout:?}: \
                 the name lookup timed out"
            )));
        }
        Ok(Err(err)) => {
            return Err(DataGovError::validation_error(format!(
                "download URL host `{host}` does not resolve: {err}"
            )));
        }
        Ok(Ok(addresses)) => addresses,
    };

    for address in resolved {
        if let Some(message) = address_refusal(&host, address.ip(), allow_private) {
            return Err(DataGovError::validation_error(message));
        }
    }
    Ok(())
}

/// Fetch `url`, following redirects here so that every hop is checked.
///
/// reqwest's own redirect handling runs in a synchronous callback, which cannot
/// resolve a name and therefore cannot judge a hop whose host is one. This
/// follows the chain instead, and puts every hop - the first and the last
/// included - through the same [`check_download_url`] before the request for it
/// is made.
///
/// Downloads are GET requests, and every status in [`REDIRECT_STATUSES`] either
/// preserves GET (307, 308) or reduces to GET (301, 302, 303), so the method is
/// the same on every hop. A `Location` is resolved against the URL it arrived
/// on, so a relative reference reaches the host it actually names.
///
/// # The client must not follow redirects itself
///
/// `http_client` has to be built with [`reqwest::redirect::Policy::none`]. A
/// client that follows redirects internally reaches the next hop without this
/// loop seeing it, which is the whole hole this exists to close - and
/// [`GuardedResolver`] does not cover for it, because reqwest skips the
/// resolver entirely when the hop names a literal address.
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when a hop is refused, when a
/// `Location` cannot be resolved, or when the chain passes
/// [`MAX_REDIRECT_HOPS`], and [`DataGovError::HttpError`] when a request fails
/// for a transport reason.
///
/// `lookup_timeout` is forwarded to [`check_download_url`] and so bounds the
/// name lookup of every hop, not only the first.
///
/// # A limit worth stating
///
/// This closes the case where a redirect to a name was followed with nothing
/// having judged the name. It does not make a hostile proxy safe. With a proxy
/// configured, the name is resolved twice - once here, and once by the proxy -
/// and the two answers can differ, exactly as they can differ between this
/// check and the connection reqwest opens without a proxy. Judging the name
/// ourselves is what a client at this layer can do; guaranteeing that the proxy
/// judged the same name the same way is not.
pub(crate) async fn fetch_checked(
    http_client: &reqwest::Client,
    url: &str,
    allow_private: bool,
    lookup_timeout: Duration,
) -> Result<reqwest::Response> {
    let mut target = Url::parse(url).map_err(|err| {
        DataGovError::validation_error(format!("download URL `{url}` does not parse: {err}"))
    })?;
    let mut hops = 0usize;

    loop {
        check_download_url(target.as_str(), allow_private, lookup_timeout).await?;

        let response =
            http_client
                .get(target.clone())
                .send()
                .await
                .map_err(|err| match refusal_in(&err) {
                    // The resolver decides refusals where a `DataGovError` cannot
                    // be returned, and attaches the reason to the transport error.
                    Some(message) => DataGovError::validation_error(message),
                    None => DataGovError::from(err),
                })?;

        let Some(next) = redirect_target(&response, &target)? else {
            return Ok(response);
        };

        hops += 1;
        if hops > MAX_REDIRECT_HOPS {
            return Err(DataGovError::validation_error(format!(
                "download of `{url}` abandoned after {MAX_REDIRECT_HOPS} redirects"
            )));
        }
        target = next;
    }
}

/// Where `response` points next, or `None` when it is not a usable redirect.
///
/// A 3xx carrying no `Location` names nowhere to go, so it is handed back as an
/// ordinary response and reported as the failed download it is.
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when the `Location` is not text or
/// does not resolve against `current`.
fn redirect_target(response: &reqwest::Response, current: &Url) -> Result<Option<Url>> {
    if !REDIRECT_STATUSES.contains(&response.status().as_u16()) {
        return Ok(None);
    }
    let Some(value) = response.headers().get(reqwest::header::LOCATION) else {
        return Ok(None);
    };
    let location = value.to_str().map_err(|_| {
        DataGovError::validation_error(format!(
            "redirect from `{current}` carries a Location header that is not text"
        ))
    })?;
    let next = current.join(location).map_err(|err| {
        DataGovError::validation_error(format!(
            "redirect from `{current}` to `{location}` does not resolve: {err}"
        ))
    })?;
    Ok(Some(next))
}

/// Recover a refusal that a transport error is carrying as its cause.
///
/// The redirect policy and the DNS resolver decide refusals in places that
/// cannot return a [`DataGovError`]. Both attach a [`RefusedDestination`],
/// which this finds so the caller can report a validation failure with the
/// real reason instead of a bare "error sending request".
pub(crate) fn refusal_in(error: &(dyn StdError + 'static)) -> Option<String> {
    let mut current = Some(error);
    while let Some(err) = current {
        if let Some(refusal) = err.downcast_ref::<RefusedDestination>() {
            return Some(refusal.0.clone());
        }
        current = err.source();
    }
    None
}

/// Confirm `candidate` names a file directly inside `root`, and nowhere else.
///
/// This is the backstop behind the filename sanitizing, not a replacement for
/// it: it holds even if the name a caller supplies stops being reduced.
///
/// The check runs twice. The lexical pass rejects an absolute path or a
/// parent-directory step before any directory is created for it. The second
/// pass canonicalizes both sides, which is what catches a symbolic link
/// pointing out of the tree, and needs `root` to exist already.
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when `candidate` resolves outside
/// `root`, and [`DataGovError::IoError`] when either path cannot be resolved.
pub(crate) async fn ensure_inside(root: &Path, candidate: &Path) -> Result<()> {
    let outside = || {
        DataGovError::validation_error(format!(
            "download path `{}` is outside the chosen directory `{}`",
            candidate.display(),
            root.display()
        ))
    };

    direct_child(root, candidate)?;

    let resolved_root = tokio::fs::canonicalize(root).await?;
    let resolved_parent = match tokio::fs::canonicalize(candidate).await {
        // Something is already at the destination. Resolve it, because a write
        // follows a symbolic link left there and would land at its target.
        Ok(existing) => existing.parent().map(Path::to_path_buf),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Some(tokio::fs::canonicalize(candidate.parent().unwrap_or(root)).await?)
        }
        Err(err) => return Err(err.into()),
    };

    if resolved_parent.as_deref() != Some(resolved_root.as_path()) {
        return Err(outside());
    }
    Ok(())
}

/// Reduce a string to a single filesystem path component.
///
/// Path separators become `_`, every character outside alphanumerics, `-`, `_`
/// and `.` is dropped, and every parent-directory sequence is collapsed to `_`.
///
/// The result is never `.` or `..`, and never carries a separator or a `..`, so
/// joining it onto a directory can only ever name a file inside that directory.
/// It may be empty, when nothing usable survived: a caller that needs a name
/// has to supply its own default.
///
/// # Examples
///
/// ```rust
/// # use data_gov::util::sanitize_path_component;
/// assert_eq!(sanitize_path_component("my-dataset_2024.csv"), "my-dataset_2024.csv");
/// assert_eq!(sanitize_path_component("../../etc/passwd"), "____etc_passwd");
/// // The `!` is dropped, and the dots it separated must not become `..`.
/// assert_eq!(sanitize_path_component(".!."), "_");
/// assert_eq!(sanitize_path_component("!@#$%"), "");
/// ```
pub fn sanitize_path_component(s: &str) -> String {
    // Filter first. Dropping a character can bring the characters either side
    // of it together, so a collapse that runs before the filter is undone by
    // it: `.!.` lost its `!` after `..` had already been dealt with, and came
    // back out as `..`.
    let mut reduced: String = s
        // A separator becomes `_` rather than vanishing, so `a/b` stays two
        // readable parts instead of collapsing into one word.
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();

    // Collapse until the result stops changing. `..` cannot reappear while the
    // replacement is `_`, but the loop is the guarantee rather than the
    // replacement text being what it happens to be today.
    while reduced.contains("..") {
        reduced = reduced.replace("..", "_");
    }

    // `.` and `..` are instructions to the filesystem, not names. `..` cannot
    // reach here after the collapse; both are stated so the postcondition
    // holds at the boundary rather than by inference from the loop above.
    if reduced == "." || reduced == ".." {
        return String::new();
    }
    reduced
}

/// Join `component` onto `root` and confirm the result stays directly inside it.
///
/// This is the check that has to run at every place a derived name is joined
/// onto a directory the caller chose. [`sanitize_path_component`] is what makes
/// the join safe; this is what makes it *checked*, so a change to the reduction
/// cannot silently move a download somewhere else.
///
/// # Errors
///
/// Returns [`DataGovError::ValidationError`] when `component` is empty,
/// absolute, or resolves anywhere other than a direct child of `root`.
///
/// # Examples
///
/// ```rust
/// # use std::path::{Path, PathBuf};
/// # use data_gov::util::join_inside;
/// let chosen = Path::new("/tmp/downloads");
/// assert_eq!(
///     join_inside(chosen, "climate-data").unwrap(),
///     PathBuf::from("/tmp/downloads/climate-data")
/// );
/// assert!(join_inside(chosen, "..").is_err());
/// ```
pub fn join_inside(root: &Path, component: &str) -> Result<std::path::PathBuf> {
    let candidate = root.join(component);
    direct_child(root, &candidate)?;
    Ok(candidate)
}

/// Fail unless `candidate` is `root` plus exactly one plain component.
///
/// Lexical only: it needs neither path to exist, which is what lets it run
/// before a directory is created for the name it is judging.
fn direct_child(root: &Path, candidate: &Path) -> Result<()> {
    let outside = || {
        DataGovError::validation_error(format!(
            "download path `{}` is outside the chosen directory `{}`",
            candidate.display(),
            root.display()
        ))
    };

    let relative = candidate.strip_prefix(root).map_err(|_| outside())?;
    let mut parts = relative.components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(outside());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// A lookup bound no test means to reach.
    const GENEROUS: Duration = Duration::from_secs(300);

    /// Every address here is drawn from the RFC that reserves the range, not
    /// from the implementation: RFC 1918 (private), RFC 3927 and RFC 4291
    /// (link-local), RFC 6598 (carrier-grade NAT), RFC 4193 (unique-local).
    const RESERVED: [(&str, BlockedRange); 20] = [
        ("127.0.0.1", BlockedRange::Loopback),
        ("127.255.255.254", BlockedRange::Loopback),
        ("0.0.0.0", BlockedRange::Unspecified),
        ("169.254.169.254", BlockedRange::LinkLocal),
        ("169.254.0.1", BlockedRange::LinkLocal),
        ("169.254.255.255", BlockedRange::LinkLocal),
        ("10.0.0.1", BlockedRange::Private),
        ("10.255.255.255", BlockedRange::Private),
        ("172.16.0.1", BlockedRange::Private),
        ("172.31.255.255", BlockedRange::Private),
        ("192.168.1.1", BlockedRange::Private),
        ("100.64.0.1", BlockedRange::CarrierGradeNat),
        ("100.127.255.255", BlockedRange::CarrierGradeNat),
        ("::1", BlockedRange::Loopback),
        ("::", BlockedRange::Unspecified),
        ("fe80::1", BlockedRange::LinkLocal),
        ("febf::1", BlockedRange::LinkLocal),
        ("fc00::1", BlockedRange::UniqueLocal),
        ("fdff::1", BlockedRange::UniqueLocal),
        // An IPv4 address in an IPv6 costume still routes as IPv4.
        ("::ffff:169.254.169.254", BlockedRange::LinkLocal),
    ];

    /// Public addresses that sit just outside a reserved range, so an
    /// off-by-one mask would show up here rather than passing unnoticed.
    const ROUTABLE: [&str; 10] = [
        "8.8.8.8",
        "1.1.1.1",
        "126.255.255.255",
        "128.0.0.1",
        "169.253.255.255",
        "169.255.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "100.63.255.255",
        "100.128.0.0",
    ];

    fn ip(text: &str) -> IpAddr {
        IpAddr::from_str(text).expect("test address must parse")
    }

    #[test]
    fn reserved_addresses_are_classified_by_their_range() {
        for (text, expected) in RESERVED {
            assert_eq!(
                classify_address(ip(text)),
                Some(expected),
                "{text} belongs to {expected:?}"
            );
        }
    }

    #[test]
    fn routable_addresses_are_not_classified() {
        for text in ROUTABLE {
            assert_eq!(
                classify_address(ip(text)),
                None,
                "{text} is routable and must be reachable"
            );
        }
    }

    #[test]
    fn every_reserved_address_is_refused_by_default() {
        for (text, _) in RESERVED {
            assert!(
                address_refusal("host.example", ip(text), false).is_some(),
                "{text} must be refused when the opt-in is off"
            );
        }
    }

    #[test]
    fn the_opt_in_reaches_every_range_except_link_local() {
        for (text, range) in RESERVED {
            let refusal = address_refusal("host.example", ip(text), true);
            if range == BlockedRange::LinkLocal {
                assert!(
                    refusal.is_some(),
                    "{text} carries instance metadata and must stay refused"
                );
            } else {
                assert!(
                    refusal.is_none(),
                    "{text} must be reachable once private downloads are allowed, got {refusal:?}"
                );
            }
        }
    }

    #[test]
    fn a_refusal_names_the_host_and_the_address() {
        let message = address_refusal("metadata.example", ip("169.254.169.254"), false)
            .expect("a link-local address is refused");
        assert!(message.contains("metadata.example"), "got: {message}");
        assert!(message.contains("169.254.169.254"), "got: {message}");
        assert!(message.contains("link-local"), "got: {message}");
    }

    #[test]
    fn only_http_and_https_pass_the_scheme_check() {
        for scheme in ["ftp", "file", "gopher", "data", "javascript"] {
            let url = Url::parse(&format!("{scheme}://example.com/x")).expect("parses");
            let error = check_url_without_dns(&url, true).expect_err("must be refused");
            assert!(error.contains(scheme), "got: {error}");
        }
        for scheme in ["http", "https"] {
            let url = Url::parse(&format!("{scheme}://example.com/x")).expect("parses");
            assert_eq!(
                check_url_without_dns(&url, false).expect("must be accepted"),
                Some("example.com".to_string()),
                "a named host is handed back for resolution"
            );
        }
    }

    #[test]
    fn a_literal_address_is_judged_without_a_lookup() {
        let url = Url::parse("http://169.254.169.254/latest/meta-data/").expect("parses");
        let error = check_url_without_dns(&url, false).expect_err("must be refused");
        assert!(error.contains("169.254.169.254"), "got: {error}");

        let url = Url::parse("http://[::ffff:127.0.0.1]/x").expect("parses");
        check_url_without_dns(&url, false).expect_err("a mapped loopback must be refused");

        let url = Url::parse("http://93.184.216.34/x").expect("parses");
        assert_eq!(
            check_url_without_dns(&url, false).expect("a routable address is accepted"),
            None,
            "a literal address needs no further resolution"
        );
    }

    #[tokio::test]
    async fn check_download_url_refuses_a_name_that_resolves_to_loopback() {
        let error = check_download_url("http://localhost/data.csv", false, GENEROUS)
            .await
            .expect_err("localhost resolves to loopback and must be refused");
        assert!(error.to_string().contains("localhost"), "got: {error}");
    }

    #[tokio::test]
    async fn check_download_url_rejects_text_that_is_not_a_url() {
        let error = check_download_url("not a url", false, GENEROUS)
            .await
            .expect_err("unparseable text must be refused");
        assert!(matches!(error, DataGovError::ValidationError { .. }));
    }

    /// A name lookup that answers after `delay`, standing in for a real one.
    ///
    /// The delay is a `tokio::time::sleep`, so under a paused clock it costs no
    /// real time and the outcome is decided by which of the two timers - this
    /// one or the bound - is nearer, never by how loaded the machine is.
    async fn lookup_answering_after(
        delay: Duration,
        address: &str,
    ) -> std::io::Result<std::vec::IntoIter<SocketAddr>> {
        let address: SocketAddr = address.parse().expect("the test address must parse");
        tokio::time::sleep(delay).await;
        Ok(vec![address].into_iter())
    }

    /// A name lookup that fails the way an absent name does.
    async fn lookup_failing_with_no_such_host() -> std::io::Result<std::vec::IntoIter<SocketAddr>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such host",
        ))
    }

    /// A name lookup that must never be reached.
    async fn lookup_that_must_not_run() -> std::io::Result<std::vec::IntoIter<SocketAddr>> {
        panic!("this URL must be judged without a lookup");
    }

    /// The bound exists so a name server that never answers cannot hold a
    /// download open. Without it nothing in the download path ends this wait:
    /// the lookup runs outside reqwest, so `connect_timeout` and `read_timeout`
    /// both miss it.
    #[tokio::test(start_paused = true)]
    async fn a_lookup_that_outlasts_the_bound_is_cut_off() {
        let error = check_download_url_with(
            "http://silent.example/data.csv",
            false,
            Duration::from_secs(5),
            |_host, _port| lookup_answering_after(Duration::from_secs(3600), "93.184.216.34:80"),
        )
        .await
        .expect_err("a lookup that outlasts the bound must be cut off");

        let message = error.to_string();
        assert!(
            message.contains("silent.example"),
            "the refusal must name the host an operator has to go and look at, got: {message}"
        );
        assert!(
            message.contains("timed out"),
            "a lookup that ran out of time must say so, got: {message}"
        );
    }

    /// The bound that decides has to be the configured one, not a constant.
    ///
    /// The same lookup is driven twice and only the bound differs, so nothing
    /// but the configured value can account for the two outcomes. The quoted
    /// figure is checked too: an operator reading the message has to be able to
    /// tell which setting cut the lookup off.
    #[tokio::test(start_paused = true)]
    async fn the_configured_bound_is_the_one_that_decides() {
        const LOOKUP_TAKES: Duration = Duration::from_secs(30);

        let cut_off = check_download_url_with(
            "http://slow.example/data.csv",
            false,
            Duration::from_secs(5),
            |_host, _port| lookup_answering_after(LOOKUP_TAKES, "93.184.216.34:80"),
        )
        .await
        .expect_err("a five second bound is shorter than a thirty second lookup");
        assert!(
            cut_off.to_string().contains("5s"),
            "the message must quote the bound that fired, got: {cut_off}"
        );

        check_download_url_with(
            "http://slow.example/data.csv",
            false,
            Duration::from_secs(300),
            |_host, _port| lookup_answering_after(LOOKUP_TAKES, "93.184.216.34:80"),
        )
        .await
        .expect("a bound longer than the lookup must let the same lookup through");
    }

    /// A lookup that ran out of time and a name that does not exist are
    /// different operational problems - a silent name server against a bad URL
    /// - so the two messages have to be told apart without guessing.
    #[tokio::test(start_paused = true)]
    async fn a_lookup_that_times_out_reads_differently_from_one_that_fails() {
        let timed_out = check_download_url_with(
            "http://silent.example/data.csv",
            false,
            Duration::from_secs(5),
            |_host, _port| lookup_answering_after(Duration::from_secs(3600), "93.184.216.34:80"),
        )
        .await
        .expect_err("the lookup outlasts the bound")
        .to_string();

        let failed = check_download_url_with(
            "http://absent.example/data.csv",
            false,
            GENEROUS,
            |_host, _port| lookup_failing_with_no_such_host(),
        )
        .await
        .expect_err("a lookup that fails is still a refusal")
        .to_string();

        assert!(timed_out.contains("timed out"), "got: {timed_out}");
        assert!(failed.contains("does not resolve"), "got: {failed}");
        assert!(
            !failed.contains("timed out"),
            "an NXDOMAIN must not read as a timeout, got: {failed}"
        );
    }

    /// Bounding the lookup must not smuggle an address past the range check.
    #[tokio::test(start_paused = true)]
    async fn a_bounded_lookup_still_judges_the_address_it_gets() {
        let error = check_download_url_with(
            "http://mirror.example/data.csv",
            false,
            GENEROUS,
            |_host, _port| lookup_answering_after(Duration::from_secs(1), "127.0.0.1:80"),
        )
        .await
        .expect_err("an answer of loopback must still be refused");
        assert!(error.to_string().contains("loopback"), "got: {error}");

        check_download_url_with(
            "http://mirror.example/data.csv",
            false,
            GENEROUS,
            |_host, _port| lookup_answering_after(Duration::from_secs(1), "93.184.216.34:80"),
        )
        .await
        .expect("a routable answer must pass");
    }

    /// The host and port the lookup is asked for are the ones the URL names,
    /// including the port the scheme implies when the URL states none.
    #[tokio::test(start_paused = true)]
    async fn the_lookup_is_asked_for_the_host_and_port_the_url_names() {
        for (url, expected_host, expected_port) in [
            ("https://mirror.example/data.csv", "mirror.example", 443u16),
            ("http://mirror.example/data.csv", "mirror.example", 80),
            (
                "http://mirror.example:8443/data.csv",
                "mirror.example",
                8443,
            ),
        ] {
            check_download_url_with(url, false, GENEROUS, |host, port| {
                assert_eq!(host, expected_host, "for {url}");
                assert_eq!(port, expected_port, "for {url}");
                lookup_answering_after(Duration::ZERO, "93.184.216.34:80")
            })
            .await
            .expect("a routable answer must pass");
        }
    }

    /// A literal address is judged without a lookup, so no bound applies to it.
    /// A zero bound would cut off any lookup at all, which is what makes it the
    /// proof that none was made.
    #[tokio::test(start_paused = true)]
    async fn a_literal_address_reaches_no_lookup_and_so_no_bound() {
        check_download_url_with(
            "http://93.184.216.34/data.csv",
            false,
            Duration::ZERO,
            |_host, _port| lookup_that_must_not_run(),
        )
        .await
        .expect("a routable literal address passes without resolving anything");
    }

    /// The resolver is the half of the check a synchronous redirect policy
    /// cannot run. It has to refuse on its own, because a redirect to a name
    /// reaches it and nothing else.
    #[tokio::test]
    async fn the_guarded_resolver_refuses_a_name_that_resolves_to_loopback() {
        let resolver = GuardedResolver::new(false);
        let name = Name::from_str("localhost").expect("localhost is a valid name");
        let error = resolver
            .resolve(name)
            .await
            .err()
            .expect("localhost resolves to loopback and must be refused");
        assert!(
            error.to_string().contains("localhost"),
            "the refusal must name the host, got: {error}"
        );
    }

    #[tokio::test]
    async fn the_guarded_resolver_honours_the_opt_in() {
        let resolver = GuardedResolver::new(true);
        let name = Name::from_str("localhost").expect("localhost is a valid name");
        let addresses: Vec<SocketAddr> = resolver
            .resolve(name)
            .await
            .expect("the opt-in must let a local mirror resolve")
            .collect();
        assert!(
            !addresses.is_empty(),
            "the resolver must hand back the addresses it approved"
        );
        // Not merely "some addresses": the ones it hands back have to be the
        // ones it was asked about, or the opt-in would be indistinguishable
        // from a resolver that answers with anything at all.
        for address in &addresses {
            assert!(
                address.ip().is_loopback(),
                "localhost must resolve to loopback, got {address}"
            );
        }
    }

    /// The resolver's refusal has to survive the trip out through reqwest.
    ///
    /// It is decided in a callback that cannot return a [`DataGovError`], so it
    /// travels as the cause of a transport error and [`refusal_in`] is what
    /// turns it back into a reason a caller can read. Nothing else proves that
    /// shape end to end: [`check_download_url`] judges every name a test can
    /// construct before the resolver is ever consulted, so the resolver only
    /// decides a case - an answer that changes between the check and the
    /// connection - that a test cannot stage.
    #[tokio::test]
    async fn a_resolver_refusal_travels_out_through_reqwest() {
        let client = reqwest::Client::builder()
            .dns_resolver(GuardedResolver::new(false))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("test client must build");

        let error = client
            .get("http://localhost/data.csv")
            .send()
            .await
            .expect_err("a name resolving to loopback must be refused by the resolver");

        let recovered = refusal_in(&error)
            .unwrap_or_else(|| panic!("the refusal must survive as a cause, got: {error:?}"));
        assert!(
            recovered.contains("localhost") && recovered.contains("loopback"),
            "the recovered refusal must name the host and the range, got: {recovered}"
        );
    }

    #[test]
    fn join_inside_accepts_a_plain_component() {
        let joined = join_inside(Path::new("/tmp/downloads"), "climate-data")
            .expect("a plain component names a directory inside the chosen one");
        assert_eq!(
            joined,
            std::path::PathBuf::from("/tmp/downloads/climate-data")
        );
    }

    /// Every one of these is a way a component leaves the directory it is
    /// joined onto, including the two the reduction is supposed to remove.
    /// The check does not depend on the reduction having removed them.
    #[test]
    fn join_inside_rejects_every_component_that_leaves_the_directory() {
        for component in [
            "..",
            "../escaped",
            "a/../..",
            "sub/dir",
            "/etc/cron.d",
            "",
            ".",
            "./.",
        ] {
            let outcome = join_inside(Path::new("/tmp/downloads"), component);
            assert!(
                matches!(outcome, Err(DataGovError::ValidationError { .. })),
                "component {component:?} must be refused, got: {outcome:?}"
            );
        }
    }

    #[test]
    fn join_inside_names_both_paths_when_it_refuses() {
        let error = join_inside(Path::new("/tmp/downloads"), "..")
            .expect_err("a parent-directory step must be refused");
        let message = error.to_string();
        assert!(message.contains("/tmp/downloads"), "got: {message}");
    }

    #[tokio::test]
    async fn ensure_inside_accepts_a_file_directly_in_the_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        ensure_inside(tmp.path(), &tmp.path().join("report.csv"))
            .await
            .expect("a plain child of the directory is inside it");
    }

    #[tokio::test]
    async fn ensure_inside_rejects_a_parent_directory_step() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("chosen");
        tokio::fs::create_dir_all(&root).await.expect("root");
        let error = ensure_inside(&root, &root.join("..").join("escaped.csv"))
            .await
            .expect_err("a parent-directory step leaves the directory");
        assert!(matches!(error, DataGovError::ValidationError { .. }));
    }

    #[tokio::test]
    async fn ensure_inside_rejects_an_absolute_path_elsewhere() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("chosen");
        tokio::fs::create_dir_all(&root).await.expect("root");
        let error = ensure_inside(&root, Path::new("/etc/cron.d/evil"))
            .await
            .expect_err("an absolute path is not inside the directory");
        assert!(matches!(error, DataGovError::ValidationError { .. }));
    }

    #[tokio::test]
    async fn ensure_inside_rejects_a_nested_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        ensure_inside(tmp.path(), &tmp.path().join("sub").join("report.csv"))
            .await
            .expect_err("only a file directly in the directory is accepted");
    }

    /// A symbolic link already at the destination is followed by the write, so
    /// it has to be resolved here. This is the case the lexical pass cannot
    /// see, and the reason the check resolves the path against the filesystem.
    #[cfg(unix)]
    #[tokio::test]
    async fn ensure_inside_rejects_a_destination_symlinked_out_of_the_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("chosen");
        let outside = tmp.path().join("elsewhere");
        tokio::fs::create_dir_all(&root).await.expect("root");
        tokio::fs::create_dir_all(&outside).await.expect("outside");

        let target = outside.join("victim.txt");
        tokio::fs::write(&target, b"original")
            .await
            .expect("target");

        let destination = root.join("report.csv");
        std::os::unix::fs::symlink(&target, &destination).expect("symlink");

        let error = ensure_inside(&root, &destination)
            .await
            .expect_err("a destination linked out of the directory must be refused");
        assert!(
            matches!(error, DataGovError::ValidationError { .. }),
            "got {error:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ensure_inside_accepts_a_destination_symlinked_within_the_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("chosen");
        tokio::fs::create_dir_all(&root).await.expect("root");

        let target = root.join("actual.csv");
        tokio::fs::write(&target, b"original")
            .await
            .expect("target");
        let destination = root.join("alias.csv");
        std::os::unix::fs::symlink(&target, &destination).expect("symlink");

        ensure_inside(&root, &destination)
            .await
            .expect("a link that stays inside the directory is not an escape");
    }

    #[test]
    fn a_refusal_is_recovered_from_a_cause_chain() {
        #[derive(Debug)]
        struct Transport(RefusedDestination);
        impl fmt::Display for Transport {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("error sending request")
            }
        }
        impl StdError for Transport {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(&self.0)
            }
        }

        let wrapped = Transport(RefusedDestination::new("destination refused"));
        assert_eq!(
            refusal_in(&wrapped),
            Some("destination refused".to_string()),
            "a refusal buried in the cause chain must surface"
        );

        let plain = std::io::Error::other("connection reset");
        assert_eq!(
            refusal_in(&plain),
            None,
            "an ordinary transport failure must not be reported as a refusal"
        );
    }

    #[test]
    fn test_sanitize_removes_path_traversal() {
        assert_eq!(
            sanitize_path_component("../../etc/passwd"),
            "____etc_passwd"
        );
    }

    #[test]
    fn test_sanitize_removes_backslash() {
        assert_eq!(sanitize_path_component("foo\\bar"), "foo_bar");
    }

    #[test]
    fn test_sanitize_preserves_safe_chars() {
        assert_eq!(
            sanitize_path_component("my-dataset_2024.csv"),
            "my-dataset_2024.csv"
        );
    }

    #[test]
    fn test_sanitize_strips_special_chars() {
        assert_eq!(sanitize_path_component("hello world!@#"), "helloworld");
    }

    #[test]
    fn test_sanitize_empty_string() {
        assert_eq!(sanitize_path_component(""), "");
    }

    /// `.` and `..` are instructions to the filesystem, not names. A caller
    /// that joins the result onto a directory must never be handed one.
    #[test]
    fn test_sanitize_never_returns_a_directory_alias() {
        for input in [".", "..", ".!.", "./.", ". .", "..!..", "  .  .  ", ".\0."] {
            let out = sanitize_path_component(input);
            assert!(
                out != "." && out != "..",
                "sanitizing {input:?} produced {out:?}, which names a directory rather than a file"
            );
        }
    }

    /// The character between the two dots is stripped, and stripping it is what
    /// brings the dots together. Every one of these is a separate way in.
    #[test]
    fn test_sanitize_cannot_rebuild_a_traversal_from_stripped_characters() {
        for stripped in [
            "!", " ", "\0", "\n", "\r", "\t", "%", "#", "@", "*", "?", ":", ";", "\"", "'", "|",
            "<", ">", "\u{202e}", "\u{ff0f}", "\u{2215}",
        ] {
            let input = format!("report.{stripped}.csv");
            let out = sanitize_path_component(&input);
            assert!(
                !out.contains(".."),
                "sanitizing {input:?} produced {out:?}, which carries a parent-directory reference"
            );

            let bare = format!(".{stripped}.");
            let out = sanitize_path_component(&bare);
            assert!(
                !out.contains(".."),
                "sanitizing {bare:?} produced {out:?}, which carries a parent-directory reference"
            );
        }
    }

    /// A sanitized name is joined onto a directory, so it has to be one plain
    /// component. This states that over the whole result, not over one shape.
    #[test]
    fn test_sanitize_always_yields_at_most_one_plain_component() {
        for input in [
            "../../etc/passwd",
            ".!.",
            "..!..",
            "....",
            "a/../b",
            "/etc/passwd",
            "C:\\Windows\\evil",
            ".\u{ff0f}.",
        ] {
            let out = sanitize_path_component(input);
            if out.is_empty() {
                continue;
            }
            assert!(
                !out.contains('/') && !out.contains('\\'),
                "sanitizing {input:?} produced {out:?}, which carries a path separator"
            );
            assert!(
                !out.contains(".."),
                "sanitizing {input:?} produced {out:?}, which carries a parent-directory reference"
            );
            let mut parts = Path::new(&out).components();
            assert!(
                matches!(parts.next(), Some(Component::Normal(_))),
                "sanitizing {input:?} produced {out:?}, which does not start with a plain component"
            );
            assert!(
                parts.next().is_none(),
                "sanitizing {input:?} produced {out:?}, which is more than one component"
            );
        }
    }

    #[test]
    fn test_sanitize_single_dot_yields_nothing_usable() {
        assert_eq!(
            sanitize_path_component("."),
            "",
            "a lone dot names the current directory, so nothing usable survives it"
        );
    }

    #[test]
    fn test_sanitize_hidden_file_prefix_preserved() {
        assert_eq!(sanitize_path_component(".bashrc"), ".bashrc");
    }

    #[test]
    fn test_sanitize_trailing_dot_preserved() {
        assert_eq!(sanitize_path_component("file."), "file.");
    }

    #[test]
    fn test_sanitize_three_dots_replaces_leading_pair() {
        assert_eq!(sanitize_path_component("..."), "_.");
    }

    #[test]
    fn test_sanitize_four_dots_replaces_both_pairs() {
        assert_eq!(sanitize_path_component("...."), "__");
    }

    #[test]
    fn test_sanitize_embedded_parent_traversal_replaced() {
        assert_eq!(sanitize_path_component("foo..bar"), "foo_bar");
    }

    #[test]
    fn test_sanitize_preserves_unicode_letters() {
        assert_eq!(sanitize_path_component("résumé"), "résumé");
        assert_eq!(sanitize_path_component("日本語"), "日本語");
    }

    #[test]
    fn test_sanitize_only_special_chars_returns_empty() {
        assert_eq!(sanitize_path_component("!@#$%^&*()"), "");
    }

    #[test]
    fn test_sanitize_long_input_does_not_panic() {
        let long = "a".repeat(10_000);
        let result = sanitize_path_component(&long);
        assert_eq!(result.len(), 10_000);
    }

    #[test]
    fn test_sanitize_mixed_safe_and_traversal() {
        assert_eq!(
            sanitize_path_component("safe-name../evil"),
            "safe-name__evil"
        );
    }
}
