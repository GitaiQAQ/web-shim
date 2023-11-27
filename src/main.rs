#![feature(async_closure)]

use async_std::channel::bounded;
use async_std::fs::create_dir_all;
use async_std::path::Path;

use futures::{join, StreamExt};

use chromiumoxide::browser::{Browser, BrowserConfig};

use tide_tracing::TraceMiddleware;

#[macro_use]
extern crate serde_derive;
extern crate serde_qs as qs;

mod config;
mod error;
mod middleware;
mod util;
mod worker;

use config::SERVER_CONFIG;
use middleware::rate_limiting::{IpRateLimitingMiddleware, NSRateLimitingMiddleware};
use worker::screenshot::{screenshot, ScreenshotWorker};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub: String,
    username: String,
    uid: u64,
    exp: usize,
}

use tracing::{debug, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[async_std::main]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    println!("{:?}", &SERVER_CONFIG.browser.args);

    let (browser, mut handler) = Browser::launch(
        BrowserConfig::builder()
            .args(&SERVER_CONFIG.browser.args)
            .window_size(SERVER_CONFIG.browser.width, SERVER_CONFIG.browser.height)
            .viewport(None)
            .port(SERVER_CONFIG.browser.port)
            .build()
            .unwrap(),
    )
    .await
    .unwrap();

    debug!("launch chrome {:?}", browser);

    let browser_handle = async_std::task::spawn(async move { handler.next().await.unwrap() });

    async_std::task::spawn(async move {
        let (tx, rx) = bounded(1);
        for id in 0..SERVER_CONFIG.browser.pool_size.into() {
            let page = browser.new_page("about:blank").await.unwrap();
            ScreenshotWorker::new(id, page, tx.clone()).await;
        }

        loop {
            let id = rx.recv().await.unwrap();
            let page = browser.new_page("about:blank").await.unwrap();
            ScreenshotWorker::new(id, page, tx.clone()).await;
        }
    });

    let http_handle = {
        let mut app = tide::new();
        app.with(TraceMiddleware::new());
        {
            // app.with(NSRateLimitingMiddleware::from(CONFIG.http.rate_limiting));
            app.with(IpRateLimitingMiddleware::from(
                &SERVER_CONFIG.http.rate_limiting,
            ));
        }

        {
            let static_path = Path::new("static/");
            if !static_path.exists().await {
                let _ = create_dir_all(static_path).await;
            }
            app.at("/static/").serve_dir("static/")?;
        }

        info!("buckets {:?}", SERVER_CONFIG.buckets);
        for (bucket, config) in &SERVER_CONFIG.buckets {
            let rate_limiting = NSRateLimitingMiddleware::from(&config.rate_limiting);
            app.at(format!("/screenshot/{:#}/", bucket).as_str())
                .with(rate_limiting)
                .get(|req| screenshot(req, bucket));
        }

        app.listen(&SERVER_CONFIG.http.listen)
    };

    let _ = join!(browser_handle, http_handle);

    Ok(())
}
