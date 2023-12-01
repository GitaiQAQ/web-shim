use chromiumoxide::{error::CdpError, page::ScreenshotParams, Page};

use futures::lock::Mutex;
use lazy_static::lazy_static;

use opendal::raw::{build_abs_path, build_rel_path};
use tide::{log::error, Error, Redirect, Request, StatusCode};

use std::env::current_dir;
use std::fs::{create_dir_all, metadata};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use chromiumoxide_cdp::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams, NavigateParams, Viewport,
};
use futures::channel::mpsc::{unbounded, Sender, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot::{channel as oneshot_channel, Sender as OneshotSender};
use futures::StreamExt;

use serde::{Deserialize, Serialize};

use tide::log::{debug, info};

use url::Url;

lazy_static! {
    static ref SCREENSHOT_TASK_CHANNEL: (
        UnboundedSender<ScreenshotTask>,
        Mutex<UnboundedReceiver<ScreenshotTask>>
    ) = {
        let (tx, rx) = unbounded();
        (tx, Mutex::new(rx))
    };
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    sub: String,
    username: String,
    uid: u64,
    exp: usize,
}

pub struct ScreenshotWorker {}

impl ScreenshotWorker {
    pub async fn new(id: usize, page: Page, mut ptx: Sender<usize>) {
        debug!("worker {:#} create {:?}", id, page);
        tokio::task::spawn(async move {
            debug!("worker {:#} start", id);
            loop {
                let ScreenshotTask(
                    ScreenshotTaskInner {
                        full_page,
                        omit_background,
                        tx,
                        req_start,
                        bucket,
                        filename,
                    },
                    navigate_params,
                    cdp_params,
                ) = SCREENSHOT_TASK_CHANNEL.1.lock().await.next().await.unwrap();

                debug!("worker {:#} recv {:#} {:?}", id, filename, cdp_params);

                let fetch_start = req_start.elapsed();
                let filename = format!(
                    "{:#}.{:#}",
                    filename,
                    match cdp_params.format.clone() {
                        Some(CaptureScreenshotFormat::Jpeg) => "jpg",
                        Some(CaptureScreenshotFormat::Webp) => "webp",
                        _ => "png",
                    }
                )
                .to_owned();

                if let Err(cdp_error) = page.goto(navigate_params).await {
                    match cdp_error {
                        CdpError::Timeout => {
                            let _ = ptx.try_send(id).unwrap();
                        }
                        err => error!("cdp error {:?}", err),
                    }
                } else {
                    if let Ok(img_buf) = page
                        .screenshot(ScreenshotParams {
                            cdp_params,
                            full_page,
                            omit_background,
                        })
                        .await
                    {
                        let browser_dur = req_start.elapsed();
                        let file_size = &img_buf.len();
                        DAL_OP_MAP
                            .get(&bucket)
                            .unwrap()
                            .write(&filename, img_buf)
                            .await
                            .unwrap();

                        let writer_dur = req_start.elapsed();
                        debug!(
                            "worker {:#} save {:#} {:#} {:#} {:#} {:#}",
                            id,
                            &filename,
                            fetch_start.as_millis(),
                            browser_dur.as_millis(),
                            writer_dur.as_millis(),
                            file_size,
                        );

                        let file_path = build_rel_path(
                            &current_dir().unwrap().to_string_lossy(),
                            build_abs_path(
                                format!("{:#}/", DAL_OP_MAP.get(&bucket).unwrap().info().root())
                                    .as_str(),
                                &filename,
                            )
                            .as_str(),
                        );

                        let _ = tx.send(Some(
                            PresignedUrl::new(
                                &file_path,
                                &SERVER_CONFIG.buckets.get(&bucket).unwrap().access_token,
                            )
                            .to_url(),
                        ));
                    } else {
                        let _ = tx.send(None);
                    }

                    let _ = page.goto("about:blank").await;

                    continue;
                }
                break;
            }
            let _ = page.close().await;
            debug!("worker {:#} end", id);
        });
        debug!("worker {:#} created", id);
    }
}

pub async fn screenshot(req: Request<()>, bucket: &str) -> tide::Result {
    let params: ScreenshotRequestParams = req.query().unwrap();

    let filename = params.filename();

    let ScreenshotRequestParams {
        url,
        format,
        quality,
        width,
        height,
        scale,
        full_page,
        omit_background,
        ttl,
    } = params;

    for ext_name in ["png", "jpg", "webp"] {
        let file_name = format!("{:#}.{:#}", filename, ext_name);
        let file = Path::new(&file_name);
        if file.exists() {
            let _ttl = ttl.unwrap_or(60 * 5);
            if _ttl > 0 {
                if let Ok(stat) = metadata(file) {
                    if let Ok(time) = stat.modified() {
                        if SystemTime::now().duration_since(time).unwrap()
                            < Duration::from_secs(_ttl)
                        {
                            return Ok(Redirect::new(
                                PresignedUrl::new(
                                    &format!("/{:#}", &file_name),
                                    &SERVER_CONFIG.buckets.get(bucket).unwrap().access_token,
                                )
                                .to_url(),
                            )
                            .into());
                        }
                    }
                }
            }
        }
    }

    let dirpath = Path::new(&filename).parent().unwrap();
    if !dirpath.exists() {
        let _ = create_dir_all(dirpath);
    }
    let (tx, rx) = oneshot_channel();

    let now = Instant::now();
    let _ = SCREENSHOT_TASK_CHANNEL
        .0
        .unbounded_send(ScreenshotTask {
            0: ScreenshotTaskInner {
                full_page,
                omit_background,
                tx,
                req_start: Instant::now(),
                bucket: bucket.to_owned(),
                filename,
            },
            1: NavigateParams {
                url: url.to_string(),
                referrer: None,
                transition_type: None,
                frame_id: None,
                referrer_policy: None,
            },
            2: CaptureScreenshotParams {
                format: Some(format.unwrap_or(CaptureScreenshotFormat::Jpeg)),
                quality: Some(quality.unwrap_or(40).into()),
                clip: Some(Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: width.unwrap_or(1920).into(),
                    height: height.unwrap_or(1080).into(),
                    scale: Into::<f64>::into(scale.unwrap_or(5)) * 0.1 / 2.0,
                }),
                from_surface: None,
                capture_beyond_viewport: None,
            },
        })
        .unwrap();

    info!("send {:#}", now.elapsed().as_millis());

    if let Ok(Some(filename)) = rx.await {
        info!("filename {:#}", filename);
        return Ok(Redirect::new(filename).into());
    }

    Err(Error::from_str(StatusCode::InternalServerError, ""))
}

struct ScreenshotTaskInner {
    tx: OneshotSender<Option<String>>,

    bucket: String,
    filename: String,
    full_page: Option<bool>,
    omit_background: Option<bool>,

    req_start: Instant,
}

struct ScreenshotTask(ScreenshotTaskInner, NavigateParams, CaptureScreenshotParams);

use std::hash::Hash;

use crate::config::{DAL_OP_MAP, SERVER_CONFIG};
use crate::util::hash::{calculate_hash, calculate_hash_str};
use crate::util::signature_v4::PresignedUrl;

#[derive(Debug, Deserialize, Hash)]
struct ScreenshotRequestParams {
    pub url: Url,

    pub format: Option<CaptureScreenshotFormat>,
    pub quality: Option<u16>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub scale: Option<u8>,

    pub full_page: Option<bool>,
    pub omit_background: Option<bool>,

    pub ttl: Option<u64>,
}

impl ScreenshotRequestParams {
    pub fn filename(&self) -> String {
        format!(
            "{:#}/{:x}",
            calculate_hash_str(&self.url.origin().ascii_serialization()),
            calculate_hash(self)
        )
    }
}
