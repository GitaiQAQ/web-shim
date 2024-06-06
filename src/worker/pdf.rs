use async_std::task::sleep;
use chromiumoxide::{Page};


use futures::lock::Mutex;
use lazy_static::lazy_static;



use tide::{Error, Redirect, Request, StatusCode};


use std::time::{Duration};

use chromiumoxide_cdp::cdp::browser_protocol::page::{
    PrintToPdfParams, NavigateParams,
};
use futures::channel::mpsc::{unbounded, Sender, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot::{channel as oneshot_channel, Sender as OneshotSender};
use futures::StreamExt;

use serde::{Deserialize, Serialize};

use tide::log::{debug, info};

use chrono::{ TimeDelta, offset::Local};

use url::Url;

lazy_static! {
    static ref PDF_TASK_CHANNEL: (
        UnboundedSender<PDFTask>,
        Mutex<UnboundedReceiver<PDFTask>>
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

pub struct PDFWorker {}

impl PDFWorker {
    pub async fn new(id: usize, page: Page, ptx: Sender<usize>) {
        debug!("worker {:#} create {:?}", id, page);
        tokio::task::spawn(async move {
            debug!("worker {:#} start", id);
            loop {
                if let Some(PDFTask(tx, inner, navigate_params, cdp_params)) =
                    PDF_TASK_CHANNEL.1.lock().await.next().await
                {
                    match worker(id, &page, inner, navigate_params, cdp_params).await {
                        Ok(uri) => {
                            tx.send(Some(uri));
                        }
                        Err(_) => {
                            tx.send(None);
                        }
                    }
                }
            }
            let _ = ptx.try_send(id).unwrap();
            let _ = page.close().await;
            debug!("worker {:#} end", id);
        });
        debug!("worker {:#} created", id);
    }
}

pub async fn worker(
    id: usize,
    page: &Page,
    inner: PDFTaskInner,
    navigate_params: NavigateParams,
    cdp_params: PrintToPdfParams,
) -> Result<String, ()> {
    debug!("worker {:#} recv {:#} {:?}", id, inner.filename, cdp_params);
    let op = DAL_OP_MAP.get(&inner.bucket).unwrap();
    let filename = format!(
        "{:#}.{:#}",
        inner.filename,
        "pdf"
    )
    .to_owned();

    let _ = page.goto(navigate_params).await.unwrap();

    sleep(Duration::from_secs(10)).await;

    let img_buf = page
        .pdf(PrintToPdfParams {
            landscape: None,
            display_header_footer: None,
            print_background: None,
            scale: Some(1.0),
            paper_width: None,
            paper_height: None,
            margin_top: None,
            margin_bottom: None,
            margin_left: None,
            margin_right: None,
            page_ranges: None,
            header_template: None,
            footer_template: None,
            prefer_css_page_size: None,
            transfer_mode: None,
        })
        .await
        .unwrap();

    let file_size = &img_buf.len();

    op.write(&filename, img_buf).await;

    let signed_url = signed_url(op, &filename, &inner.bucket).await.unwrap();

    debug!(
        "worker {:#} save {:#} {:#}",
        id,
        &filename,
        file_size,
    );

    page.goto("about:blank").await.unwrap();

    return Ok(signed_url);
}

pub async fn pdf(req: Request<()>, bucket: &str) -> tide::Result {
    let params: PDFRequestQSParams = req.query().unwrap();

    let filename = params.filename();
    let path = params.path();
    let op = DAL_OP_MAP.get(bucket).unwrap();

    let PDFRequestQSParams {
        url,
        scale,
        omit_background,
        ttl,
    } = params;

    if op.is_exist(&path).await.unwrap() && ttl.is_some() {
      if op.stat(&path).await.unwrap().last_modified().unwrap().checked_add_signed(TimeDelta::new(ttl.unwrap().try_into().unwrap(), 0).unwrap()).unwrap() >= Local::now() {
        let signed_url = signed_url(op, &path, bucket).await.unwrap();
        return Ok(Redirect::new(signed_url).into());
      }
    }

    let (tx, rx) = oneshot_channel();

    let default_pdf_task_params = &SERVER_CONFIG
        .buckets
        .get(bucket)
        .unwrap()
        .pdf_task_params
        .clone()
        .unwrap();

    let _ = PDF_TASK_CHANNEL
        .0
        .unbounded_send(PDFTask {
            0: tx,
            1: PDFTaskInner {
                bucket: bucket.to_owned(),
                filename,
            },
            2: NavigateParams {
                url: url.to_string(),
                referrer: None,
                transition_type: None,
                frame_id: None,
                referrer_policy: None,
            },
            3: PrintToPdfParams {
              landscape: None,
              display_header_footer: None,
              print_background: omit_background,
              scale: Some(Into::<f64>::into(
                    scale.unwrap_or(default_pdf_task_params.scale.unwrap()),
                ) / 10.0),
              paper_width: None,
              paper_height: None,
              margin_top: None,
              margin_bottom: None,
              margin_left: None,
              margin_right: None,
              page_ranges: None,
              header_template: None,
              footer_template: None,
              prefer_css_page_size: None,
              transfer_mode: None,
            },
        })
        .unwrap();

    if let Ok(Some(filename)) = rx.await {
        info!("redirect to {:#}", filename);
        return Ok(Redirect::new(filename).into());
    }

    Err(Error::from_str(StatusCode::InternalServerError, ""))
}

struct PDFTaskInner {
    bucket: String,
    filename: String,
}

struct PDFTask(
    OneshotSender<Option<String>>,
    PDFTaskInner,
    NavigateParams,
    PrintToPdfParams,
);

use std::hash::Hash;

use crate::config::{DAL_OP_MAP, SERVER_CONFIG};
use crate::util::hash::{calculate_hash, calculate_hash_str};
use crate::util::signature_v4::{signed_url};

#[derive(Debug, Serialize, Deserialize, Clone, Hash)]
pub struct PDFRequestQSParams {
    pub url: Url,

    pub scale: Option<u8>,
    pub ttl: Option<u64>,

    pub omit_background: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Hash)]
pub struct PDFRequestParams {
    #[serde(default = "default_scale")]
    pub scale: Option<u8>,
    #[serde(default = "default_ttl")]
    pub ttl: Option<u64>,

    pub omit_background: Option<bool>,
}

impl PDFRequestQSParams {
    pub fn filename(&self) -> String {
        format!(
            "{:#}/{:x}",
            calculate_hash_str(&self.url.origin().ascii_serialization()),
            calculate_hash(self)
        )
    }

    pub fn path(&self) -> String {
        format!(
            "{:#}.{:#}",
            self.filename(),
            "pdf"
        )
    }
}

pub fn default_buckets_pdf_task_params() -> Option<PDFRequestParams> {
    Some(PDFRequestParams {
        scale: default_scale(),
        omit_background: None,
        ttl: default_ttl(),
    })
}

fn default_scale() -> Option<u8> {
    Some(5)
}

fn default_ttl() -> Option<u64> {
    Some(60)
}
