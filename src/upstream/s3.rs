use crate::{
    config::UpstreamS3Config,
    upstream::{Upstream, UpstreamError, UpstreamResource, UpstreamResourceHeaders},
};

pub struct S3Upstream {
    s3: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
}

impl S3Upstream {
    pub async fn new(config: UpstreamS3Config) -> anyhow::Result<Self> {
        let mut aws = aws_config::from_env();
        if let Some(profile) = config.profile {
            aws = aws.profile_name(profile);
        }
        if let Some(region) = config.region {
            aws = aws.region(aws_config::Region::new(region));
        }
        let aws = aws.load().await;

        let mut s3 = aws_sdk_s3::config::Builder::from(&aws);
        if let Some(endpoint_url) = config.endpoint_url {
            s3 = s3.endpoint_url(endpoint_url);
        }
        let s3 = s3.build();
        let s3 = aws_sdk_s3::Client::from_conf(s3);

        Ok(Self {
            s3,
            bucket: config.bucket,
            prefix: config.prefix,
        })
    }
}

#[async_trait::async_trait]
impl Upstream for S3Upstream {
    async fn get(&self, path: &str) -> Result<Option<UpstreamResource>, UpstreamError> {
        // Trim leading '/' characters
        let path = path.trim_start_matches('/');

        let key = format!("{}{path}", self.prefix);

        tracing::trace!(bucket = self.bucket, key, "requesting S3 object");

        let response = self
            .s3
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                if let Some(aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_)) =
                    error.as_service_error()
                {
                    return Ok(None);
                } else {
                    return Err(Box::new(error).into());
                }
            }
        };

        let content_type = response.content_type().and_then(|content_type| {
            axum::http::HeaderValue::from_str(content_type)
                .inspect_err(|error| {
                    tracing::warn!(
                        "invlaid Content-Type header value from S3: {content_type}: {error}"
                    )
                })
                .ok()
        });
        let body = axum_body_from_s3_response(response);
        let resource = UpstreamResource {
            body,
            headers: UpstreamResourceHeaders { content_type },
        };
        Ok(Some(resource))
    }
}

fn axum_body_from_s3_response(
    response: aws_sdk_s3::operation::get_object::GetObjectOutput,
) -> axum::body::Body {
    let response_size: Option<u64> = response
        .content_length()
        .and_then(|content_length| u64::try_from(content_length).ok());
    let response_reader = response.body.into_async_read();
    let response_stream = tokio_util::io::ReaderStream::new(response_reader);

    let size_hint = response_size.map(http_body::SizeHint::with_exact);
    let body =
        crate::response::BodyWithSize::new(crate::response::StreamBody::new(response_stream))
            .with_size_hint(size_hint);
    axum::body::Body::new(body)
}
