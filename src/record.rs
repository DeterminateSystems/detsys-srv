//! SRV records.

use std::{cmp::Reverse, fmt::Display};

use http::uri::Scheme;
use rand::Rng;
use url::Url;

/// Representation of types that contain the fields of a SRV record.
pub trait SrvRecord {
    /// Type representing the SRV record's target. Must implement `Display` so
    /// it can be used to create a `Uri`.
    type Target: Display + ?Sized;

    /// Gets a SRV record's target.
    fn target(&self) -> &Self::Target;

    /// Gets a SRV record's port.
    fn port(&self) -> u16;

    /// Gets a SRV record's priority.
    fn priority(&self) -> u16;

    /// Gets a SRV record's weight.
    fn weight(&self) -> u16;

    /// Parses a SRV record into a URI with a given scheme (e.g. https)
    fn parse(&self, scheme: Scheme) -> Result<Url, url::ParseError> {
        // We do this funny parsing of a bogus URL and then set the
        // properties to get the benefits of parsing each field, since
        // url::Url doesn't support constructing a URL from parts.
        //
        // If we were to format!() together the scheme, target, and port
        // in one shot, the `target` could ostensibly contain
        // `foo.com:123/bar`.
        // Then the port would be appended to the end of that, which would
        // not set the port.
        let mut url = url::Url::parse("http://example.com")?;
        url.set_scheme(scheme.as_str())
            .expect("...Scheme supports HTTP and HTTPS, and that is it.");
        url.set_host(Some(&self.target().to_string()))?;
        url.set_port(Some(self.port()))
            .map_err(|_| url::ParseError::SetHostOnCannotBeABaseUrl)?;

        Ok(url)
    }

    /// Generates a key to sort a SRV record by priority and weight per RFC 2782.
    fn sort_key(&self, rng: impl Rng) -> (u16, Reverse<u32>) {
        sort_key(self.priority(), self.weight(), rng)
    }
}

/// Generates a key to sort a SRV record by priority and weight per RFC 2782.
pub(crate) fn sort_key(priority: u16, weight: u16, mut rng: impl Rng) -> (u16, Reverse<u32>) {
    // Sort ascending by priority, then descending (hence `Reverse`) by randomized weight
    let rand = rng.random::<u16>() as u32;
    (priority, Reverse(weight as u32 * rand))
}
