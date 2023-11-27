use std::include_bytes;

pub static RSA_KEY: &[u8] = include_bytes!("./key.pem");
pub static PK8_KEY: &[u8] = include_bytes!("./pk8key.pem");
