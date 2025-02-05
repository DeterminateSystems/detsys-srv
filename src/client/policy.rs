use crate::{resolver::SrvResolver, Error, SrvClient, SrvRecord};
use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use std::sync::Arc;
use url::Url;

pub use super::Cache;

/// Policy for [`SrvClient`] to use when selecting SRV targets to recommend.
#[async_trait]
pub trait Policy: Sized {
    /// Type of item stored in a client's cache.
    type CacheItem;

    /// Iterator of indices used to order cache items.
    type Ordering: Iterator<Item = usize>;

    /// Obtains a refreshed cache for a client.
    async fn refresh_cache<Resolver: SrvResolver>(
        &self,
        client: &SrvClient<Resolver, Self>,
    ) -> Result<Cache<Self::CacheItem>, Error<Resolver::Error>>;

    /// Creates an iterator of indices corresponding to cache items in the
    /// order a [`SrvClient`] should try using them to perform an operation.
    fn order(&self, items: &[Self::CacheItem]) -> Self::Ordering;

    /// Converts a reference to a cached item into a reference to a [`Url`].
    fn cache_item_to_uri(item: &Self::CacheItem) -> &Url;

    /// Makes any policy adjustments following a successful execution on `url`.
    #[allow(unused_variables)]
    fn note_success(&self, url: &Url) {}

    /// Makes any policy adjustments following a failed execution on `uri`.
    #[allow(unused_variables)]
    fn note_failure(&self, url: &Url) {}
}

/// Policy that selects targets based on past successes--if a target was used
/// successfully in a past execution, it will be recommended first.
#[derive(Default)]
pub struct Affinity {
    last_working_target: ArcSwapOption<Url>,
}

#[async_trait]
impl Policy for Affinity {
    type CacheItem = Url;
    type Ordering = AffinityUrlIter;

    async fn refresh_cache<Resolver: SrvResolver>(
        &self,
        client: &SrvClient<Resolver, Self>,
    ) -> Result<Cache<Self::CacheItem>, Error<Resolver::Error>> {
        let (uris, valid_until) = client.get_fresh_uri_candidates().await?;
        Ok(Cache::new(uris, valid_until))
    }

    fn order(&self, uris: &[Url]) -> Self::Ordering {
        let preferred = self.last_working_target.load();
        Affinity::uris_preferring(uris, preferred.as_deref())
    }

    fn cache_item_to_uri(item: &Self::CacheItem) -> &Url {
        item
    }

    fn note_success(&self, uri: &Url) {
        self.last_working_target.store(Some(Arc::new(uri.clone())));
    }
}

impl Affinity {
    fn uris_preferring(uris: &[Url], preferred: Option<&Url>) -> AffinityUrlIter {
        let preferred = preferred
            .and_then(|preferred| uris.as_ref().iter().position(|uri| uri == preferred))
            .unwrap_or(0);
        AffinityUrlIter {
            n: uris.len(),
            preferred,
            next: None,
        }
    }
}

/// Iterator over [`Url`]s based on affinity. See [`Affinity`].
pub struct AffinityUrlIter {
    /// Number of uris in the cache.e
    n: usize,
    /// Index of the URI to produce first (i.e. the preferred URL).
    /// `0` if the first is preferred or there is no preferred URL at all.
    preferred: usize,
    /// Index of the next URI to be produced.
    /// If `None`, the preferred URI will be produced.
    next: Option<usize>,
}

impl Iterator for AffinityUrlIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let (idx, next) = match self.next {
            // If no URIs have been produced, produce the preferred URI then go back to the first
            None => (self.preferred, 0),
            // If `preferred` is next, skip past it since it was produced already (`self.next != None`)
            Some(next) if next == self.preferred => (next + 1, next + 2),
            // Otherwise, advance normally
            Some(next) => (next, next + 1),
        };
        self.next = Some(next);
        if idx < self.n {
            Some(idx)
        } else {
            None
        }
    }
}

/// Policy that selects targets based on the algorithm in RFC 2782, reshuffling
/// by weight for each selection.
#[derive(Default)]
pub struct Rfc2782;

/// Representation of a SRV record with its target and port parsed into a [`Url`].
pub struct ParsedRecord {
    uri: Url,
    priority: u16,
    weight: u16,
}

impl ParsedRecord {
    fn new<Record: SrvRecord>(record: &Record, uri: Url) -> Self {
        Self {
            uri,
            priority: record.priority(),
            weight: record.weight(),
        }
    }
}

#[async_trait]
impl Policy for Rfc2782 {
    type CacheItem = ParsedRecord;
    type Ordering = <Vec<usize> as IntoIterator>::IntoIter;

    async fn refresh_cache<Resolver: SrvResolver>(
        &self,
        client: &SrvClient<Resolver, Self>,
    ) -> Result<Cache<Self::CacheItem>, Error<Resolver::Error>> {
        let (records, valid_until) = client.get_srv_records().await?;
        let parsed = records
            .iter()
            .map(|record| {
                client
                    .parse_record(record)
                    .map(|uri| ParsedRecord::new(record, uri))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Cache::new(parsed, valid_until))
    }

    fn order(&self, records: &[ParsedRecord]) -> Self::Ordering {
        let mut indices = (0..records.len()).collect::<Vec<_>>();
        let mut rng = rand::rng();
        indices.sort_by_cached_key(|&idx| {
            let (priority, weight) = (records[idx].priority, records[idx].weight);
            crate::record::sort_key(priority, weight, &mut rng)
        });
        indices.into_iter()
    }

    fn cache_item_to_uri(item: &Self::CacheItem) -> &Url {
        &item.uri
    }
}

#[test]
fn affinity_uris_iter_order() {
    let google: Url = "https://google.com".parse().unwrap();
    let amazon: Url = "https://amazon.com".parse().unwrap();
    let desco: Url = "https://deshaw.com".parse().unwrap();
    let cache = vec![google.clone(), amazon.clone(), desco.clone()];
    let order = |preferred| {
        Affinity::uris_preferring(&cache, preferred)
            .map(|idx| &cache[idx])
            .collect::<Vec<_>>()
    };
    assert_eq!(order(None), vec![&google, &amazon, &desco]);
    assert_eq!(order(Some(&google)), vec![&google, &amazon, &desco]);
    assert_eq!(order(Some(&amazon)), vec![&amazon, &google, &desco]);
    assert_eq!(order(Some(&desco)), vec![&desco, &google, &amazon]);
}

#[test]
fn balance_uris_iter_order() {
    // Clippy doesn't like that Url has interior mutability and is being used
    // as a HashMap key but we aren't doing anything naughty in the test
    #[allow(clippy::mutable_key_type)]
    let mut priorities = std::collections::HashMap::new();
    priorities.insert("https://google.com".parse::<Url>().unwrap(), 2);
    priorities.insert("https://cloudflare.com".parse().unwrap(), 2);
    priorities.insert("https://amazon.com".parse().unwrap(), 1);
    priorities.insert("https://deshaw.com".parse().unwrap(), 1);

    let cache = priorities
        .iter()
        .map(|(uri, &priority)| ParsedRecord {
            uri: uri.clone(),
            priority,
            weight: rand::random::<u8>() as u16,
        })
        .collect::<Vec<_>>();

    let ordered = |iter: <Rfc2782 as Policy>::Ordering| {
        let mut last = None;
        for item in iter.map(|idx| &cache[idx]) {
            if let Some(last) = last {
                assert!(priorities[last] <= priorities[&item.uri]);
            }
            last = Some(&item.uri);
        }
    };

    for _ in 0..5 {
        ordered(Rfc2782.order(&cache));
    }
}
