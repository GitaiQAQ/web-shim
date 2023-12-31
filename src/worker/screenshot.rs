use chromiumoxide::{error::CdpError, page::ScreenshotParams, Page};

use chromiumoxide_cdp::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
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
                    let clip = &cdp_params.clip.unwrap();

                    page.execute(SetDeviceMetricsOverrideParams::new(
                        clip.width as i64,
                        clip.height as i64,
                        2.0,
                        false,
                    ))
                    .await
                    .unwrap();

                    if let Ok(img_buf) = page
                        .screenshot(ScreenshotParams {
                            cdp_params: CaptureScreenshotParams {
                                format: cdp_params.format,
                                quality: cdp_params.quality,
                                clip: Some(Viewport { ..clip.clone() }),
                                from_surface: None,
                                capture_beyond_viewport: None,
                            },
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
    let params: ScreenshotRequestQSParams = req.query().unwrap();

    let filename = params.filename();

    let ScreenshotRequestQSParams {
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

    let (tx, rx) = oneshot_channel();

    let now = Instant::now();
    let default_screenshot_task_params = &SERVER_CONFIG
        .buckets
        .get(bucket)
        .unwrap()
        .screenshot_task_params
        .clone()
        .unwrap();

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
                format: Some(
                    format.unwrap_or(default_screenshot_task_params.format.clone().unwrap()),
                ),
                quality: Some(
                    quality
                        .unwrap_or(default_screenshot_task_params.quality.unwrap())
                        .into(),
                ),
                clip: Some(Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: width
                        .unwrap_or(default_screenshot_task_params.width.unwrap())
                        .into(),
                    height: height
                        .unwrap_or(default_screenshot_task_params.height.unwrap())
                        .into(),
                    scale: Into::<f64>::into(
                        scale.unwrap_or(default_screenshot_task_params.scale.unwrap()),
                    ) / 10.0,
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

#[derive(Debug, Serialize, Deserialize, Clone, Hash)]
pub struct ScreenshotRequestQSParams {
    pub url: Url,

    pub format: Option<CaptureScreenshotFormat>,
    pub quality: Option<u16>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub scale: Option<u8>,
    pub ttl: Option<u64>,

    pub full_page: Option<bool>,
    pub omit_background: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash)]
pub struct ScreenshotRequestParams {
    #[serde(default = "default_format")]
    pub format: Option<CaptureScreenshotFormat>,
    #[serde(default = "default_quality")]
    pub quality: Option<u16>,
    #[serde(default = "default_width")]
    pub width: Option<u16>,
    #[serde(default = "default_height")]
    pub height: Option<u16>,
    #[serde(default = "default_scale")]
    pub scale: Option<u8>,
    #[serde(default = "default_ttl")]
    pub ttl: Option<u64>,

    pub full_page: Option<bool>,
    pub omit_background: Option<bool>,
}

impl ScreenshotRequestQSParams {
    pub fn filename(&self) -> String {
        format!(
            "{:#}/{:x}",
            calculate_hash_str(&self.url.origin().ascii_serialization()),
            calculate_hash(self)
        )
    }
}

pub fn default_buckets_screenshot_task_params() -> Option<ScreenshotRequestParams> {
    Some(ScreenshotRequestParams {
        format: default_format(),
        quality: default_quality(),
        width: default_width(),
        height: default_height(),
        scale: default_scale(),
        ttl: default_ttl(),
        full_page: None,
        omit_background: None,
    })
}

fn default_format() -> Option<CaptureScreenshotFormat> {
    Some(CaptureScreenshotFormat::Jpeg)
}

fn default_quality() -> Option<u16> {
    Some(40)
}

fn default_width() -> Option<u16> {
    Some(1920)
}

fn default_height() -> Option<u16> {
    Some(1080)
}

fn default_scale() -> Option<u8> {
    Some(5)
}

fn default_ttl() -> Option<u64> {
    Some(60)
}
