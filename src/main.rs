#![feature(async_closure)]

use futures::{channel::mpsc::channel, join, StreamExt};
use std::path::Path;
use std::{fs::create_dir_all, process};

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
use worker::pdf::{pdf, PDFWorker};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub: String,
    username: String,
    uid: u64,
    exp: usize,
}

use tracing::{debug, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::{config::DAL_OP_MAP, middleware::access_control::LfsAccessControlMiddleware};

#[tokio::main]
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

    let browser_handle = tokio::task::spawn(async move { handler.next().await.unwrap() });

    tokio::task::spawn(async move {
        let (tx, mut rx) = channel(1);
        for id in 0..SERVER_CONFIG.browser.pool_size.into() {
            ScreenshotWorker::new(id, browser.new_page("about:blank").await.unwrap(), tx.clone()).await;
        }

        PDFWorker::new((SERVER_CONFIG.browser.pool_size + 1).into(), browser.new_page("about:blank").await.unwrap(), tx.clone()).await;

        loop {
            let id = rx.next().await.unwrap();
            let page = browser.new_page("about:blank").await.unwrap();
            if (id > SERVER_CONFIG.browser.pool_size.into()) {
                PDFWorker::new(id, page, tx.clone()).await;
            } else {
                ScreenshotWorker::new(id, page, tx.clone()).await;
            }
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
            if !static_path.exists() {
                let _ = create_dir_all(static_path);
            }
        }

        info!("buckets {:?}", SERVER_CONFIG.buckets);
        for (bucket, config) in &SERVER_CONFIG.buckets {
            DAL_OP_MAP.get(bucket).unwrap().create_dir("/").await?;
            let rate_limiting = NSRateLimitingMiddleware::from(&config.rate_limiting);
            app.at(format!("/screenshot/{:#}/", bucket).as_str())
                .with(rate_limiting)
                .get(|req| screenshot(req, bucket));
            
            let pdf_rate_limiting = NSRateLimitingMiddleware::from(&config.rate_limiting);
            app.at(format!("/pdf/{:#}/", bucket).as_str())
                .with(pdf_rate_limiting)
                .get(|req| pdf(req, bucket));
        }

        app.at("/static/")
            .with(LfsAccessControlMiddleware {
                access_tokens: SERVER_CONFIG
                    .buckets
                    .iter()
                    .map(|(_k, v)| v.access_token.clone())
                    .collect(),
            })
            .serve_dir("static/")?;

        app.at("/stats").get(|_| async {
            let pid_map = util::pstree::build_process_tree();
            Ok(format!(
                "## pstree\n {}",
                util::pstree::format_node(&(pid_map.get(&process::id()).unwrap()), 0, &pid_map)
            ))
        });

        app.listen(&SERVER_CONFIG.http.listen)
    };

    let _ = join!(browser_handle, http_handle);

    Ok(())
}
