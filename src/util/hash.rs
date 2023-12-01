use sha1::{Digest, Sha1};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub fn calculate_hash_str<T: Hash>(t: &T) -> String {
    format!("{:x}", calculate_hash(t))
}

/// verify sha256 checksum string
pub fn is_sha256_checksum(s: &str) -> bool {
    let is_lowercase_hex = |&c: &u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
    s.len() == 64 && s.as_bytes().iter().all(is_lowercase_hex)
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut m = Sha1::new();
    m.update(data.as_ref());
    format!("{:x}", m.finalize())
}
