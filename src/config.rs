use lazy_static::lazy_static;
use opendal::{Operator, Scheme};

use crate::middleware::rate_limiting::RateLimitingConfig;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

lazy_static! {
    pub static ref SERVER_CONFIG: ServerConfig = {
        if let Ok(file) = std::fs::File::open("./config.json") {
            let reader = std::io::BufReader::new(file);
            serde_json::from_reader(reader).unwrap()
        } else {
            let default_config = ServerConfig::default();
            if let Ok(file) = std::fs::File::create("./config.json") {
                let writer = std::io::BufWriter::new(file);
                serde_json::to_writer_pretty(writer, &default_config).unwrap();
            }
            default_config
        }
    };
    pub static ref DAL_OP_MAP: HashMap<String, Operator> = {
        let mut map = HashMap::new();
        for (bucket, config) in &SERVER_CONFIG.buckets {
            map.insert(
                bucket.clone(),
                Operator::via_map(Scheme::Fs, config.dal.clone()).unwrap(),
            );
        }
        map
    };
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ServerConfig {
    #[serde(default)]
    pub browser: BrowserConfig,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default = "default_bucket")]
    pub buckets: HashMap<String, Bucket>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            browser: BrowserConfig::default(),
            http: HttpConfig::default(),
            buckets: default_bucket(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BrowserConfig {
    #[serde(default = "default_browser_args")]
    pub args: Vec<String>,
    #[serde(default = "default_browser_width")]
    pub width: u32,
    #[serde(default = "default_browser_height")]
    pub height: u32,
    #[serde(default)]
    pub port: u16,
    #[serde(default = "default_browser_pool_size")]
    pub pool_size: u8,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        BrowserConfig {
            args: default_browser_args(),
            width: default_browser_width(),
            height: default_browser_height(),
            port: 0,
            pool_size: default_browser_pool_size(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HttpConfig {
    #[serde(default = "default_http_listen")]
    pub listen: String,
    #[serde(default = "default_http_rate_limiting")]
    pub rate_limiting: RateLimitingConfig,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            listen: default_http_listen(),
            rate_limiting: default_http_rate_limiting(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Bucket {
    #[serde(default = "default_buckets_access_token")]
    pub access_token: String,
    #[serde(default = "default_buckets_rate_limiting")]
    pub rate_limiting: RateLimitingConfig,
    #[serde(default = "default_buckets_dal")]
    pub dal: HashMap<String, String>,
}

impl Default for Bucket {
    fn default() -> Self {
        let dal = default_buckets_dal();
        Bucket {
            access_token: default_buckets_access_token(),
            rate_limiting: default_buckets_rate_limiting(),
            dal: dal.clone(),
        }
    }
}

fn default_bucket() -> HashMap<String, Bucket> {
    HashMap::from([("default".to_owned(), Bucket::default())])
}

/// These are passed to the Chrome binary by default.
/// Via https://github.com/puppeteer/puppeteer/blob/4846b8723cf20d3551c0d755df394cc5e0c82a94/src/node/Launcher.ts#L157
static DEFAULT_PUPPETEER_ARGS: [&str; 25] = [
    "--disable-background-networking",
    "--enable-features=NetworkService,NetworkServiceInProcess",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-component-extensions-with-background-pages",
    "--disable-default-apps",
    "--disable-dev-shm-usage",
    "--disable-extensions",
    "--disable-features=TranslateUI",
    "--disable-hang-monitor",
    "--disable-ipc-flooding-protection",
    "--disable-popup-blocking",
    "--disable-prompt-on-repost",
    "--disable-renderer-backgrounding",
    "--disable-sync",
    "--force-color-profile=srgb",
    "--metrics-recording-only",
    "--no-first-run",
    "--enable-automation",
    "--password-store=basic",
    "--use-mock-keychain",
    "--enable-blink-features=IdleDetection",
    "--lang=en_US",
];

static DEFAULT_ARGS: [&str; 10] = [
    "--disable-gpu",
    "--no-default-browser-check",
    "--hide-scrollbars",
    "--no-sandbox",
    "--disable-namespace-sandbox",
    "--disable-setuid-sandbox",
    "--block-new-web-contents",
    "--force-device-scale-factor=2",
    "--headless",
    "--single-process",
];

fn default_browser_args() -> Vec<String> {
    [
        &(DEFAULT_PUPPETEER_ARGS.map(|f| f.to_owned()))[..],
        &(DEFAULT_ARGS.map(|f| f.to_owned()))[..],
    ]
    .concat()
}

fn default_browser_width() -> u32 {
    1920
}

fn default_browser_height() -> u32 {
    1080
}

fn default_browser_pool_size() -> u8 {
    2
}

fn default_http_listen() -> String {
    "0.0.0.0:2023".to_owned()
}

fn default_http_rate_limiting() -> RateLimitingConfig {
    RateLimitingConfig::QPS(100)
}

fn default_buckets_access_token() -> String {
    "".to_owned()
}

fn default_buckets_rate_limiting() -> RateLimitingConfig {
    RateLimitingConfig::QPM(15)
}

fn default_buckets_dal() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("root".to_string(), "./static".to_string());
    map
}
