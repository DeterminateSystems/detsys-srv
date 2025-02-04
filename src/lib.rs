#![deny(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/*!
Rust client for communicating with services located by DNS SRV records.

# Introduction

SRV Records, as defined in [RFC 2782](https://tools.ietf.org/html/rfc2782),
are DNS records of the form

`_Service._Proto.Name TTL Class SRV Priority Weight Port Target`

For instance, a DNS server might respond with the following SRV records for
`_http._tcp.example.com`:

```text
_http._tcp.example.com. 60 IN SRV 1 100 443 test1.example.com.
_http._tcp.example.com. 60 IN SRV 2 50  443 test2.example.com.
_http._tcp.example.com. 60 IN SRV 2 50  443 test3.example.com.
```

A client wanting to communicate with this example service would first try to
communicate with `test1.example.com:443` (the record with the lowest
priority), then with the other two (in a random order, since they are of the
same priority) should the first be unavailable.

`srv-rs` handles the lookup and caching of SRV records as well as the ordered
selection of targets to use for communication with SRV-located services.

[`SrvClient::new`] creates a client (that should be reused to take advantage of
caching) for communicating with the service located by `_http._tcp.example.com`.
[`SrvClient::execute`] takes in a future-producing closure (emulating async
closures, which are currently unstable) and executes the closure on a series of
targets parsed from the discovered SRV records, stopping and returning the
first `Ok` or last `Err` it obtains.

# Alternative Resolvers and Target Selection Policies

`srv-rs` provides multiple resolver backends for SRV lookup and by default uses
a target selection policy that maintains affinity for the last target it has
used successfully. Both of these behaviors can be changed by implementing the
[`SrvResolver`] and [`Policy`] traits, respectively.

The provided resolver backends are enabled by the following features:

- `trust-dns` (via [`trust_dns_resolver::AsyncResolver`])

[`SrvResolver`]: resolver::SrvResolver
[`Policy`]: policy::Policy
*/

mod client;
pub use client::{policy, Error, SrvClient};

mod record;
pub use record::SrvRecord;

pub mod resolver;

#[doc(hidden)]
pub const EXAMPLE_SRV: &str = "_detsys_ids._tcp.install.determinate.systems.";
#[doc(hidden)]
pub fn example_fallback() -> url::Url {
    url::Url::parse("https://install.determinate.systems.").unwrap()
}
