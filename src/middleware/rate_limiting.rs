use governor::{
    clock::{Clock, DefaultClock},
    state::keyed::DefaultKeyedStateStore,
    Quota, RateLimiter,
};
use lazy_static::lazy_static;
use std::{
    convert::TryInto,
    error::Error,
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
};
use tide::{
    http::StatusCode, log::debug, utils::async_trait, Middleware, Next, Request, Response, Result,
};

lazy_static! {
    static ref CLOCK: DefaultClock = DefaultClock::default();
}

#[derive(Debug, Clone)]
pub struct IpRateLimitingMiddleware {
    limiter: Arc<RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>>,
}

impl Fns for IpRateLimitingMiddleware {
    type Key = IpAddr;

    fn new(quota: Quota) -> Self {
        Self {
            limiter: Arc::new(RateLimiter::<IpAddr, _, _>::keyed(quota)),
        }
    }
}

impl From<RateLimitingConfig> for IpRateLimitingMiddleware {
    fn from(value: RateLimitingConfig) -> Self {
        match value {
            RateLimitingConfig::QPS(t) => Self::per_second(t.to_owned()).unwrap(),
            RateLimitingConfig::QPM(t) => Self::per_minute(t.to_owned()).unwrap(),
            RateLimitingConfig::QPH(t) => Self::per_hour(t.to_owned()).unwrap(),
            _ => Self::per_second(30).unwrap(),
        }
    }
}

use crate::error::GlobalError;

impl From<&RateLimitingConfig> for IpRateLimitingMiddleware {
    fn from(value: &RateLimitingConfig) -> Self {
        match value {
            RateLimitingConfig::QPS(t) => Self::per_second(t.to_owned()),
            RateLimitingConfig::QPM(t) => Self::per_minute(t.to_owned()),
            RateLimitingConfig::QPH(t) => Self::per_hour(t.to_owned()),
        }
        .unwrap()
    }
}

impl TryFrom<&Option<RateLimitingConfig>> for IpRateLimitingMiddleware {
    type Error = GlobalError;

    fn try_from(value: &Option<RateLimitingConfig>) -> std::result::Result<Self, Self::Error> {
        if let Ok(rate_limiting) = match value {
            Some(RateLimitingConfig::QPS(t)) => Self::per_second(t.to_owned()),
            Some(RateLimitingConfig::QPM(t)) => Self::per_minute(t.to_owned()),
            Some(RateLimitingConfig::QPH(t)) => Self::per_hour(t.to_owned()),
            None => todo!(),
        } {
            Ok(rate_limiting)
        } else {
            Err(GlobalError::Unknown)
        }
    }
}

#[async_trait]
impl<State: Clone + Send + Sync + 'static> Middleware<State> for IpRateLimitingMiddleware {
    async fn handle(&self, req: Request<State>, next: Next<'_, State>) -> tide::Result {
        let remote = req.remote().ok_or_else(|| {
            tide::Error::from_str(
                StatusCode::InternalServerError,
                "failed to get request remote address",
            )
        })?;
        let remote: IpAddr = match remote.parse::<SocketAddr>() {
            Ok(r) => r.ip(),
            Err(_) => remote.parse()?,
        };
        debug!("remote: {}", remote);

        match self.limiter.check_key(&remote) {
            Ok(_) => {
                debug!("allowing remote {}", remote);
                Ok(next.run(req).await)
            }
            Err(negative) => {
                let wait_time = negative.wait_time_from(CLOCK.now());
                let res = Response::builder(StatusCode::TooManyRequests)
                    .header(
                        tide::http::headers::RETRY_AFTER,
                        wait_time.as_secs().to_string(),
                    )
                    .build();
                debug!(
                    "blocking address {} for {} seconds",
                    remote,
                    wait_time.as_secs()
                );
                Ok(res)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct NSRateLimitingMiddleware {
    namespace: Option<String>,
    limiter: Arc<RateLimiter<Option<String>, DefaultKeyedStateStore<Option<String>>, DefaultClock>>,
}

impl Fns for NSRateLimitingMiddleware {
    type Key = Option<String>;

    fn new(quota: Quota) -> Self {
        Self {
            namespace: None,
            limiter: Arc::new(RateLimiter::<Option<String>, _, _>::keyed(quota)),
        }
    }
}

impl From<&RateLimitingConfig> for NSRateLimitingMiddleware {
    fn from(value: &RateLimitingConfig) -> Self {
        match value {
            RateLimitingConfig::QPS(t) => Self::per_second(t.to_owned()),
            RateLimitingConfig::QPM(t) => Self::per_minute(t.to_owned()),
            RateLimitingConfig::QPH(t) => Self::per_hour(t.to_owned()),
        }
        .unwrap()
    }
}

impl TryFrom<&Option<RateLimitingConfig>> for NSRateLimitingMiddleware {
    type Error = GlobalError;

    fn try_from(value: &Option<RateLimitingConfig>) -> std::result::Result<Self, Self::Error> {
        if let Ok(rate_limiting) = match value {
            Some(RateLimitingConfig::QPS(t)) => Self::per_second(t.to_owned()),
            Some(RateLimitingConfig::QPM(t)) => Self::per_minute(t.to_owned()),
            Some(RateLimitingConfig::QPH(t)) => Self::per_hour(t.to_owned()),
            None => todo!(),
        } {
            Ok(rate_limiting)
        } else {
            Err(GlobalError::Unknown)
        }
    }
}

#[async_trait]
impl<State: Clone + Send + Sync + 'static> Middleware<State> for NSRateLimitingMiddleware {
    async fn handle(&self, req: Request<State>, next: Next<'_, State>) -> tide::Result {
        match self.limiter.check_key(&self.namespace) {
            Ok(_) => Ok(next.run(req).await),
            Err(negative) => {
                let wait_time = negative.wait_time_from(CLOCK.now());
                let res = Response::builder(StatusCode::TooManyRequests)
                    .header(
                        tide::http::headers::RETRY_AFTER,
                        wait_time.as_secs().to_string(),
                    )
                    .build();
                debug!(
                    "blocking namespace {:?} for {} seconds",
                    &self.namespace,
                    wait_time.as_secs()
                );
                Ok(res)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", content = "times")]
pub enum RateLimitingConfig {
    QPH(u32),
    QPM(u32),
    QPS(u32),
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        RateLimitingConfig::QPS(100)
    }
}

pub trait Fns: Sized {
    type Key;
    fn new(quota: Quota) -> Self;

    fn per_second<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self::new(Quota::per_second(times.try_into()?)))
    }

    fn per_minute<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self::new(Quota::per_minute(times.try_into()?)))
    }

    fn per_hour<T>(times: T) -> Result<Self>
    where
        T: TryInto<NonZeroU32>,
        T::Error: Error + Send + Sync + 'static,
    {
        Ok(Self::new(Quota::per_minute(times.try_into()?)))
    }
}
