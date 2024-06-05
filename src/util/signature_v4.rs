use std::{env::current_dir, time::{Duration, SystemTime, UNIX_EPOCH}};

use opendal::raw::{build_abs_path, build_rel_path};
use serde::{Deserialize, Serialize};
use tide::Request;

use crate::config::SERVER_CONFIG;

use super::{
    hash::{is_sha256_checksum, sha1_hex},
    time::now,
};

/// query strings of a presigned url
#[derive(Debug, Serialize, Deserialize)]
pub struct PresignedQs {
    /// X-Amz-Algorithm
    x_amz_algorithm: String,
    /// X-Amz-Credential
    x_amz_credential: String,
    /// X-Amz-Date
    x_amz_date: u64,
    /// X-Amz-Expires
    x_amz_expires: u64,
    /// X-Amz-SignedHeaders
    // x_amz_signed_headers: String,
    /// X-Amz-Signature
    x_amz_signature: String,
}

/// Access key ID and the scope information, which includes the date, Region, and service that were used to calculate the signature.
///
/// This string has the following form:
/// `<your-access-key-id>/<date>/<aws-region>/<aws-service>/aws4_request`
///
/// See [sigv4-auth-using-authorization-header](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-auth-using-authorization-header.html)
// #[derive(Debug, Serialize, Deserialize)]
// pub struct CredentialV4<'a> {
/// access key id
// pub access_key_id: &'a str,
// <date> value is specified using YYYYMMDD format.
// pub date: &'a str,
// region
// pub aws_region: &'a str,
// <aws-service> value is `s3` when sending request to Amazon S3.
// pub aws_service: &'a str,
// }

// /// x-amz-date
// #[derive(Debug, Clone, Copy)]
// pub struct AmzDate {
//     /// year
//     year: u32,
//     /// month
//     month: u32,
//     /// day
//     day: u32,
//     /// hour
//     hour: u32,
//     /// minute
//     minute: u32,
//     /// second
//     second: u32,
// }

/// presigned url information
#[derive(Debug, Serialize, Deserialize)]
pub struct PresignedUrl {
    pub path: String,
    /// X-Amz-Algorithm
    x_amz_algorithm: String,
    /// X-Amz-Credential
    x_amz_credential: String,
    /// X-Amz-Date
    x_amz_date: u64,
    /// X-Amz-Expires
    x_amz_expires: u64,
    // X-Amz-SignedHeaders
    // x_amz_signed_headers: String,
}

/// `ParsePresignedUrlError`
#[allow(missing_copy_implementations)]
#[derive(Debug, thiserror::Error)] // Why? See `crate::path::ParseS3PathError`.
#[error("ParsePresignedUrlError")]
pub struct ParsePresignedUrlError {
    msg: String,
}

impl PresignedUrl {
    /// parse `PresignedUrl` from query
    pub fn from_req<S>(req: &Request<S>) -> Result<String, ParsePresignedUrlError> {
        let path = req.url().path();
        if let Ok(PresignedQs {
            x_amz_algorithm,
            x_amz_credential,
            x_amz_date,
            x_amz_expires,
            x_amz_signature,
        }) = req.query::<PresignedQs>()
        {
            // if !is_sha256_checksum(&x_amz_signature) {
            //     return Err(ParsePresignedUrlError {
            //         msg: "invalid signature format".to_owned(),
            //     });
            // }

            let ts_now = now();

            if (ts_now < x_amz_date) {
                return Err(ParsePresignedUrlError {
                    msg: "WTF".to_owned(),
                });
            }

            if (x_amz_date + x_amz_expires < ts_now) {
                return Err(ParsePresignedUrlError {
                    msg: "timeout".to_owned(),
                });
            }

            let signed_url = Self {
                path: path.to_owned(),
                x_amz_algorithm: x_amz_algorithm.to_owned(),
                x_amz_credential: x_amz_credential.to_owned(),
                x_amz_date,
                x_amz_expires,
            };

            if !signed_url.sign().eq(&x_amz_signature) {
                return Err(ParsePresignedUrlError {
                    msg: "invalid signature".to_owned(),
                });
            }

            return Ok(x_amz_credential);
        }
        return Err(ParsePresignedUrlError {
            msg: "invalid query".to_owned(),
        });
    }

    pub fn sign(&self) -> String {
        sha1_hex(serde_json::to_string(self).unwrap().as_bytes())
    }

    pub fn new(path: &str, access_key_id: &str) -> Self {
        Self {
            path: path.to_owned(),
            x_amz_algorithm: "AWS4-HMAC-SHA256".to_owned(),
            x_amz_credential: access_key_id.to_owned(),
            x_amz_date: now(),
            x_amz_expires: Duration::from_secs_f32(3600.0).as_secs(),
        }
    }

    pub fn to_qs(&self) -> Result<std::string::String, qs::Error> {
        let x_amz_signature = self.sign();
        let PresignedUrl {
            path,
            x_amz_algorithm,
            x_amz_credential,
            x_amz_date,
            x_amz_expires,
        } = self;

        serde_qs::to_string(&PresignedQs {
            x_amz_algorithm: x_amz_algorithm.to_string(),
            x_amz_credential: x_amz_credential.to_string(),
            x_amz_date: *x_amz_date,
            x_amz_expires: *x_amz_expires,
            x_amz_signature: x_amz_signature,
        })
    }

    pub fn to_url(&self) -> String {
        format!("{:#}?{:#}", self.path, self.to_qs().unwrap())
    }
}

pub async fn signed_url(op: &opendal::Operator, filename: &String, bucket: &str) -> Result<String, ()> {
    let signed_url = match op.info().scheme() {
        opendal::Scheme::Fs => {
            let file_path = build_rel_path(
                &current_dir().unwrap().to_string_lossy(),
                build_abs_path(
                    format!("{:#}/", op.info().root()).as_str(),
                    filename,
                )
                .as_str()
            );
            PresignedUrl::new(
                &file_path,
                &SERVER_CONFIG.buckets.get(bucket).unwrap().access_token,
            )
            .to_url()
        
        },
        _ => {
            op
            .presign_read(filename, Duration::from_secs(3600))
            .await
            .unwrap()
            .uri()
            .to_string()
        }
    };
    Ok(signed_url)
}