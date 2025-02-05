//! Clients based on SRV lookups.

use crate::{resolver::SrvResolver, SrvRecord};
use arc_swap::ArcSwap;
use http::uri::Scheme;
use std::{fmt::Debug, future::Future, sync::Arc, time::Instant};
use url::Url;

mod cache;
pub use cache::Cache;

/// SRV target selection policies.
pub mod policy;

/// Errors encountered by a [`SrvClient`].
#[derive(Debug, thiserror::Error)]
pub enum Error<Lookup: Debug> {
    /// SRV lookup errors
    #[error("SRV lookup error")]
    Lookup(Lookup),
    /// SRV record parsing errors
    #[error("building url from SRV record: {0}")]
    RecordParsing(#[from] url::ParseError),
    /// Produced when there are no SRV targets for a client to use
    #[error("no SRV targets to use")]
    NoTargets,
}

/// Client for intelligently performing operations on a service located by SRV records.
///
/// # Usage
///
/// After being created by [`SrvClient::new`] or [`SrvClient::new_with_resolver`],
/// operations can be performed on the service pointed to by a [`SrvClient`] with
/// the [`execute`] and [`execute_stream`] methods.
///
/// ## DNS Resolvers
///
/// The resolver used to lookup SRV records is determined by a client's
/// [`SrvResolver`], and can be set with [`SrvClient::resolver`].
///
/// ## SRV Target Selection Policies
///
/// SRV target selection order is determined by a client's [`Policy`],
/// and can be set with [`SrvClient::policy`].
///
/// [`execute`]: SrvClient::execute()
/// [`execute_stream`]: SrvClient::execute_stream()
/// [`Policy`]: policy::Policy
#[derive(Debug)]
pub struct SrvClient<Resolver, Policy: policy::Policy = policy::Affinity> {
    srv: String,
    fallback: url::Url,
    allowed_suffixes: Option<Vec<url::Host>>,
    resolver: Resolver,
    http_scheme: Scheme,
    path_prefix: String,
    policy: Policy,
    cache: ArcSwap<Cache<Policy::CacheItem>>,
}

impl<Resolver: Default, Policy: policy::Policy + Default> SrvClient<Resolver, Policy> {
    /// Creates a new client for communicating with services located by `srv_name`.
    ///
    pub fn new(
        srv_name: impl ToString,
        fallback: url::Url,
        allowed_suffixes: Option<Vec<url::Host>>,
    ) -> Self {
        Self::new_with_resolver(srv_name, fallback, allowed_suffixes, Resolver::default())
    }
}

impl<Resolver, Policy: policy::Policy + Default> SrvClient<Resolver, Policy> {
    /// Creates a new client for communicating with services located by `srv_name`.
    pub fn new_with_resolver(
        srv_name: impl ToString,
        fallback: url::Url,
        allowed_suffixes: Option<Vec<url::Host>>,
        resolver: Resolver,
    ) -> Self {
        Self {
            srv: srv_name.to_string(),
            fallback,
            allowed_suffixes,
            resolver,
            http_scheme: Scheme::HTTPS,
            path_prefix: String::from("/"),
            policy: Default::default(),
            cache: Default::default(),
        }
    }
}

impl<Resolver: SrvResolver, Policy: policy::Policy> SrvClient<Resolver, Policy> {
    /// Gets a fresh set of SRV records from a client's DNS resolver, returning
    /// them along with the time they're valid until.
    async fn get_srv_records(
        &self,
    ) -> Result<(Vec<Resolver::Record>, Instant), Error<Resolver::Error>> {
        self.resolver
            .get_srv_records(&self.srv)
            .await
            .map_err(Error::Lookup)
    }

    /// Gets a fresh set of SRV records from a client's DNS resolver and parses
    /// their target/port pairs into URIs, which are returned along with the
    /// time they're valid until--i.e., the time a cache containing these URIs
    /// should expire.
    pub async fn get_fresh_uri_candidates(
        &self,
    ) -> Result<(Vec<Url>, Instant), Error<Resolver::Error>> {
        // Query DNS for the SRV record
        let (records, valid_until) = self.get_srv_records().await?;

        // Create URIs from SRV records
        let uri_iter = records
            .iter()
            .map(|record| self.parse_record(record))
            .filter_map(|parsed| match parsed {
                Ok(record) => Some(record),
                Err(e) => {
                    #[cfg(feature = "log")]
                    tracing::trace!(%e, "Failed to parse an SRV record");
                    None
                }
            });

        let uris = if let Some(allowed_suffixes) = &self.allowed_suffixes {
            use url::Host;

            let mut allowed_ipv4 = Vec::<&std::net::Ipv4Addr>::new();
            let mut allowed_ipv6 = Vec::<&std::net::Ipv6Addr>::new();
            let mut allowed_domains = Vec::<&str>::new();

            for suffix in allowed_suffixes {
                match suffix {
                    Host::Ipv4(ip) => {
                        allowed_ipv4.push(ip);
                    }
                    Host::Ipv6(ip) => {
                        allowed_ipv6.push(ip);
                    }
                    Host::Domain(d) => {
                        allowed_domains.push(d);
                    }
                }
            }

            uri_iter
                .filter(|record| {
                    let allow = match record.host() {
                    None => false,
                    Some(Host::Ipv4(ip)) => allowed_ipv4.contains(&&ip),
                    Some(Host::Ipv6(ip)) => allowed_ipv6.contains(&&ip),
                    Some(Host::Domain(candidate)) => allowed_domains
                        .iter()
                        .any(|allowed| candidate.ends_with(allowed)),
                };

                if !allow {
                    #[cfg(feature = "log")]
                    tracing::trace!(%record, "Rejecting SRV record because it is not allowed by the allowed suffixes");
                }

                allow
        })
                .collect::<Vec<Url>>()
        } else {
            uri_iter.collect::<Vec<Url>>()
        };

        Ok((uris, valid_until))
    }

    async fn refresh_cache(&self) -> Result<Arc<Cache<Policy::CacheItem>>, Error<Resolver::Error>> {
        let new_cache = Arc::new(self.policy.refresh_cache(self).await?);
        self.cache.store(new_cache.clone());
        Ok(new_cache)
    }

    /// Gets a client's cached items, refreshing the existing cache if it is invalid.
    async fn get_valid_cache(
        &self,
    ) -> Result<Arc<Cache<Policy::CacheItem>>, Error<Resolver::Error>> {
        match self.cache.load_full() {
            cache if cache.valid() => Ok(cache),
            _ => self.refresh_cache().await,
        }
    }

    /// Performs an operation on a client's SRV targets, producing the first
    /// successful result or the last error encountered if every execution of
    /// the operation was unsuccessful.
    ///
    pub async fn execute<T, E, Fut>(&self, func: impl FnMut(Url) -> Fut) -> Result<T, E>
    where
        E: std::error::Error,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut func = func;
        let cache = match self.get_valid_cache().await {
            Ok(c) => c,
            Err(e) => {
                #[cfg(feature = "log")]
                tracing::debug!(%e, "No valid cache");
                return func(self.fallback.clone()).await;
            }
        };

        let order = self.policy.order(cache.items());
        let cache_items = order.map(|idx| &cache.items()[idx]);

        for cache_item in cache_items.into_iter() {
            let candidate = Policy::cache_item_to_uri(cache_item);

            match func(candidate.to_owned()).await {
                Ok(res) => {
                    #[cfg(feature = "log")]
                    tracing::info!(URI = %candidate, "execution attempt succeeded");
                    self.policy.note_success(candidate);
                    return Ok(res);
                }
                Err(err) => {
                    #[cfg(feature = "log")]
                    tracing::info!(URI = %candidate, error = %err, "execution attempt failed");
                    self.policy.note_failure(candidate);
                }
            }
        }

        func(self.fallback.clone()).await
    }

    fn parse_record(&self, record: &Resolver::Record) -> Result<Url, url::ParseError> {
        record.parse(self.http_scheme.clone())
    }
}

impl<Resolver, Policy: policy::Policy> SrvClient<Resolver, Policy> {
    /// Sets the SRV name of the client.
    pub fn srv_name(self, srv_name: impl ToString) -> Self {
        Self {
            srv: srv_name.to_string(),
            ..self
        }
    }

    /// Sets the resolver of the client.
    pub fn resolver<R>(self, resolver: R) -> SrvClient<R, Policy> {
        SrvClient {
            resolver,
            cache: Default::default(),
            policy: self.policy,
            srv: self.srv,
            fallback: self.fallback,
            allowed_suffixes: self.allowed_suffixes,
            http_scheme: self.http_scheme,
            path_prefix: self.path_prefix,
        }
    }

    /// Sets the policy of the client.
    pub fn policy<P: policy::Policy>(self, policy: P) -> SrvClient<Resolver, P> {
        SrvClient {
            policy,
            cache: Default::default(),
            resolver: self.resolver,
            srv: self.srv,
            fallback: self.fallback,
            allowed_suffixes: self.allowed_suffixes,
            http_scheme: self.http_scheme,
            path_prefix: self.path_prefix,
        }
    }

    /// Sets the http scheme of the client.
    pub fn http_scheme(self, http_scheme: Scheme) -> Self {
        Self {
            http_scheme,
            ..self
        }
    }

    /// Sets the path prefix of the client.
    pub fn path_prefix(self, path_prefix: impl ToString) -> Self {
        Self {
            path_prefix: path_prefix.to_string(),
            ..self
        }
    }
}
