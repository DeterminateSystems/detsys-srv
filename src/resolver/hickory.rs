//! SRV resolver backed by [`hickory_resolver`].

use super::SrvResolver;
use crate::SrvRecord;
use async_trait::async_trait;
use hickory_resolver::{
    name_server::ConnectionProvider, proto::rr::rdata::SRV, Name, ResolveError, Resolver,
};
use std::time::Instant;

#[async_trait]
impl<P> SrvResolver for Resolver<P>
where
    P: ConnectionProvider,
{
    type Record = SRV;
    type Error = ResolveError;

    async fn get_srv_records_unordered(
        &self,
        srv: &str,
    ) -> Result<(Vec<Self::Record>, Instant), Self::Error> {
        let lookup = self.srv_lookup(srv).await?;
        let valid_until = lookup.as_lookup().valid_until();
        Ok((lookup.into_iter().collect(), valid_until))
    }
}

impl SrvRecord for SRV {
    type Target = Name;

    fn target(&self) -> &Self::Target {
        self.target()
    }

    fn port(&self) -> u16 {
        self.port()
    }

    fn priority(&self) -> u16 {
        self.priority()
    }

    fn weight(&self) -> u16 {
        self.weight()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[doc(hidden)]
    pub const EXAMPLE_SRV: &str = "_detsys_ids._tcp.install.determinate.systems.";
    #[doc(hidden)]
    pub fn example_fallback() -> url::Url {
        url::Url::parse("https://install.determinate.systems.").unwrap()
    }

    #[tokio::test]
    async fn srv_lookup() -> Result<(), ResolveError> {
        let (records, _) = Resolver::builder_tokio()?
            .build()
            .get_srv_records_unordered(EXAMPLE_SRV)
            .await?;
        assert_ne!(records.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn srv_lookup_ordered() -> Result<(), ResolveError> {
        let (records, _) = Resolver::builder_tokio()?
            .build()
            .get_srv_records(EXAMPLE_SRV)
            .await?;
        assert_ne!(records.len(), 0);
        assert!((0..records.len() - 1).all(|i| records[i].priority() <= records[i + 1].priority()));
        Ok(())
    }

    #[tokio::test]
    async fn get_fresh_uris() -> Result<(), ResolveError> {
        let resolver = Resolver::builder_tokio()?.build();
        let client = crate::SrvClient::<_>::new_with_resolver(
            EXAMPLE_SRV,
            example_fallback(),
            None,
            resolver,
        );
        let (uris, _) = client.get_fresh_uri_candidates().await.unwrap();
        assert_ne!(uris, Vec::<url::Url>::new());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_host() {
        Resolver::builder_tokio()
            .unwrap()
            .build()
            .get_srv_records("_http._tcp.foobar.deshaw.com")
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn malformed_srv_name() {
        Resolver::builder_tokio()
            .unwrap()
            .build()
            .get_srv_records("_http.foobar.deshaw.com")
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn very_malformed_srv_name() {
        Resolver::builder_tokio()
            .unwrap()
            .build()
            .get_srv_records("  @#*^[_hsd flt.com")
            .await
            .unwrap_err();
    }

    #[tokio::test]
    async fn srv_name_containing_nul_terminator() {
        Resolver::builder_tokio()
            .unwrap()
            .build()
            .get_srv_records("_http.\0_tcp.foo.com")
            .await
            .unwrap_err();
    }
}
