use tide::{http::StatusCode, log::debug, utils::async_trait, Middleware, Next, Request, Response};

use crate::util::signature_v4::PresignedUrl;

#[derive(Debug, Clone)]
pub struct LfsAccessControlMiddleware {
    pub access_tokens: Vec<String>,
}

#[async_trait]
impl<State: Clone + Send + Sync + 'static> Middleware<State> for LfsAccessControlMiddleware {
    async fn handle(&self, req: Request<State>, next: Next<'_, State>) -> tide::Result {
        match PresignedUrl::from_req(&req) {
            Ok(_) => Ok(next.run(req).await),
            Err(negative) => {
                debug!("invalid signature {:?}", negative);
                Ok(Response::builder(StatusCode::Unauthorized).build())
            }
        }
    }
}
