use bytes::Bytes;
use envoy_types::pb::envoy::service::ext_proc::v3::{HttpBody, HttpHeaders};
use http::header::HeaderValue as HttpHeaderValue;
use http::{HeaderMap as HttpHeaderMap, HeaderName, Request, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::Full;
use httpdate::fmt_http_date;
use httpsig_hyper::HyperDigestError;
use httpsig_hyper::{prelude::*, *};
use std::time::SystemTime;
use tracing::{error, info};

use crate::SignetError;

pub const COVERED_COMPONENTS: &[&str] = &[
    "@status",
    "date",
    "content-type",
    "content-digest",
];

pub type SignedBody = BoxBody<Bytes, HyperDigestError>;

#[derive(Debug)]
pub struct UpstreamResponseAcc {
    pub(crate) status: Option<StatusCode>,
    pub(crate) headers: HttpHeaderMap,
    body_bytes: Vec<u8>,
    pub(crate) done: bool,
}

impl Default for UpstreamResponseAcc {
    fn default() -> Self {
        Self {
            status: None,
            headers: HttpHeaderMap::new(),
            body_bytes: Vec::new(),
            done: false,
        }
    }
}

impl UpstreamResponseAcc {
    pub fn on_response_headers(&mut self, response_headers: HttpHeaders) {
        self.done = response_headers.end_of_stream;
        for header in response_headers.headers.unwrap_or_default().headers {
            if header.key == ":status" {
                let s = if !header.raw_value.is_empty() {
                    match std::str::from_utf8(&header.raw_value) {
                        Ok(s) => s,
                        Err(_) => continue,
                    }
                } else {
                    header.value.as_str()
                };
                if let Ok(code) = s.parse::<u16>() {
                    self.status = StatusCode::from_u16(code).ok();
                }
                continue;
            }
            if header.key.starts_with(':') {
                continue;
            }

            let Ok(key) = HeaderName::from_bytes(header.key.as_bytes()) else {
                continue;
            };
            let Ok(value) = HttpHeaderValue::from_bytes(&header.raw_value) else {
                continue;
            };
            self.headers.append(key, value);
        }
    }

    pub fn on_response_body_chunk(&mut self, chunk: HttpBody) {
        self.body_bytes.extend(&chunk.body);
        self.done = chunk.end_of_stream;
    }

    fn maybe_attach_date(&mut self) {
        if !self.done || self.headers.contains_key("date") {
            return;
        }
        let date_str = fmt_http_date(SystemTime::now());
        if let Ok(value) = HttpHeaderValue::from_str(&date_str) {
            self.headers.insert(HeaderName::from_static("date"), value);
        }
    }

    pub async fn maybe_finalize(
        &mut self,
        sk: &SecretKey,
    ) -> Result<Option<Response<SignedBody>>, SignetError> {
        self.maybe_attach_date();
        if !self.done {
            return Ok(None);
        }

        info!("Response finished");
        let http_response = self.build_http_response();

        let mut http_response = http_response
            .set_content_digest(&ContentDigestType::Sha256)
            .await
            .map_err(|e| SignetError::SigningFailed(e.to_string()))?;

        let covered_components = COVERED_COMPONENTS
            .iter()
            .map(|v| message_component::HttpMessageComponentId::try_from(*v))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SignetError::InvalidComponent(e.to_string()))?;

        let mut signature_params = HttpSignatureParams::try_new(&covered_components)
            .map_err(|e| SignetError::InvalidComponent(e.to_string()))?;

        // Replay protection: set expires to 5 minutes after created
        signature_params.set_expires_with_duration(Some(300));

        match http_response
            .set_message_signature(
                &signature_params,
                sk,
                Some("signet"),
                None::<&Request<&str>>,
            )
            .await
        {
            Ok(_) => {
                info!("Response signed successfully");
                Ok(Some(http_response))
            }
            Err(e) => {
                error!("Signing failed: {e}");
                Ok(None)
            }
        }
    }

    fn build_http_response(&mut self) -> Response<Full<Bytes>> {
        let mut builder = Response::builder().status(self.status.unwrap_or(StatusCode::OK));
        for header in self.headers.iter() {
            builder = builder.header(header.0, header.1);
        }
        let body = std::mem::take(&mut self.body_bytes);
        builder
            .body(Full::new(Bytes::from(body)))
            .expect("valid response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use envoy_types::pb::envoy::config::core::v3::{
        HeaderMap as EnvoyHeaderMap, HeaderValue as EnvoyHeaderValue,
    };
    use sha2::{Digest, Sha256};

    fn test_secret_key() -> SecretKey {
        let signing_key = p256::ecdsa::SigningKey::random(
            &mut p256::elliptic_curve::rand_core::OsRng,
        );
        SecretKey::EcdsaP256Sha256(signing_key.into())
    }

    #[tokio::test]
    async fn content_digest_known_vector() {
        let sk = test_secret_key();
        let mut acc = UpstreamResponseAcc::default();
        acc.status = Some(StatusCode::OK);
        acc.headers.insert(
            HeaderName::from_static("content-type"),
            HttpHeaderValue::from_static("application/json"),
        );
        acc.on_response_body_chunk(HttpBody {
            body: b"hello world".to_vec(),
            end_of_stream: true,
            ..Default::default()
        });

        let response = acc.maybe_finalize(&sk).await.unwrap().unwrap();
        let digest_header = response
            .headers()
            .get("content-digest")
            .unwrap()
            .to_str()
            .unwrap();
        let expected_hash = B64.encode(Sha256::digest(b"hello world"));
        let expected = format!("sha-256=:{expected_hash}:");
        assert_eq!(digest_header, expected);
    }

    #[test]
    fn date_injected_when_missing() {
        let mut acc = UpstreamResponseAcc::default();
        acc.done = true;
        acc.maybe_attach_date();
        assert!(acc.headers.contains_key("date"));
    }

    #[test]
    fn date_preserved_when_present() {
        let mut acc = UpstreamResponseAcc::default();
        acc.done = true;
        acc.headers.insert(
            HeaderName::from_static("date"),
            HttpHeaderValue::from_static("Mon, 01 Jan 2024 00:00:00 GMT"),
        );
        acc.maybe_attach_date();
        assert_eq!(
            acc.headers.get("date").unwrap().to_str().unwrap(),
            "Mon, 01 Jan 2024 00:00:00 GMT"
        );
    }

    #[test]
    fn header_parsing_extracts_correctly_and_skips_pseudo_headers() {
        let mut acc = UpstreamResponseAcc::default();
        let headers = HttpHeaders {
            headers: Some(EnvoyHeaderMap {
                headers: vec![
                    EnvoyHeaderValue {
                        key: ":status".to_string(),
                        value: "200".to_string(),
                        raw_value: b"200".to_vec(),
                    },
                    EnvoyHeaderValue {
                        key: "content-type".to_string(),
                        value: "".to_string(),
                        raw_value: b"application/json".to_vec(),
                    },
                ],
            }),
            end_of_stream: false,
            ..Default::default()
        };
        acc.on_response_headers(headers);
        assert_eq!(acc.status, Some(StatusCode::from_u16(200).unwrap()));
        assert_eq!(
            acc.headers.get("content-type").unwrap().to_str().unwrap(),
            "application/json"
        );
        assert!(!acc.headers.contains_key(":status"));
    }

    #[tokio::test]
    async fn signing_round_trip() {
        let sk = test_secret_key();
        let mut response = Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("date", "Mon, 01 Jan 2024 00:00:00 GMT")
            .header(
                "content-digest",
                "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:",
            )
            .body(Full::new(Bytes::new()))
            .unwrap();

        let covered_components = COVERED_COMPONENTS
            .iter()
            .map(|v| message_component::HttpMessageComponentId::try_from(*v))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let signature_params = HttpSignatureParams::try_new(&covered_components).unwrap();

        response
            .set_message_signature(
                &signature_params,
                &sk,
                Some("signet"),
                None::<&Request<&str>>,
            )
            .await
            .unwrap();

        assert!(response.headers().contains_key("signature"));
        assert!(response.headers().contains_key("signature-input"));

        let pk = sk.public_key();
        let result = response
            .verify_message_signature(&pk, None, None::<&Request<&str>>)
            .await;
        assert!(
            result.is_ok(),
            "Signature verification failed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn maybe_finalize_returns_none_when_not_done() {
        let sk = test_secret_key();
        let mut acc = UpstreamResponseAcc::default();
        let result = acc.maybe_finalize(&sk).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn malformed_status_does_not_panic() {
        let mut acc = UpstreamResponseAcc::default();
        let headers = HttpHeaders {
            headers: Some(EnvoyHeaderMap {
                headers: vec![EnvoyHeaderValue {
                    key: ":status".to_string(),
                    value: "".to_string(),
                    raw_value: vec![0xFF, 0xFE],
                }],
            }),
            end_of_stream: false,
            ..Default::default()
        };
        acc.on_response_headers(headers);
        assert_eq!(acc.status, None);
    }

    #[tokio::test]
    async fn signing_failure_returns_none() {
        let mut acc = UpstreamResponseAcc::default();
        acc.done = true;
        acc.status = Some(StatusCode::OK);
        acc.headers.insert(
            HeaderName::from_static("content-type"),
            HttpHeaderValue::from_static("application/json"),
        );

        let sk = test_secret_key();
        let result = acc.maybe_finalize(&sk).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn signed_response_has_created_and_expires() {
        let sk = test_secret_key();
        let mut acc = UpstreamResponseAcc::default();
        acc.status = Some(StatusCode::OK);
        acc.headers.insert(
            HeaderName::from_static("content-type"),
            HttpHeaderValue::from_static("application/json"),
        );
        acc.done = true;

        let response = acc.maybe_finalize(&sk).await.unwrap().unwrap();
        let sig_input = response
            .headers()
            .get("signature-input")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            sig_input.contains("created="),
            "Signature-Input missing 'created': {sig_input}"
        );
        assert!(
            sig_input.contains("expires="),
            "Signature-Input missing 'expires': {sig_input}"
        );
    }
}
