use std::sync::Arc;

use crate::upstream::{Upstream, UpstreamError, UpstreamResource};

pub struct FallthroughUpstream {
    upstreams: Vec<Arc<dyn Upstream + Send + Sync>>,
}

impl FallthroughUpstream {
    pub fn new(upstreams: Vec<Arc<dyn Upstream + Send + Sync>>) -> Self {
        Self { upstreams }
    }
}

#[async_trait::async_trait]
impl Upstream for FallthroughUpstream {
    async fn get(&self, path: &str) -> Result<Option<UpstreamResource>, UpstreamError> {
        let mut first_error = None;
        for (index, upstream) in self.upstreams.iter().enumerate() {
            let result = upstream.get(path).await;
            match result {
                Ok(Some(resource)) => return Ok(Some(resource)),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(index, "fallthrough upstream returned error: {error}");
                    first_error.get_or_insert(error);
                }
            }
        }

        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(None)
        }
    }
}
