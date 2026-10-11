//! Linux randomness and PKCE helpers; OpenSSL is used only for SHA-256.
use std::io::Read;
#[link(name = ":libcrypto.so.3")]
unsafe extern "C" {
    fn SHA256(data: *const u8, len: usize, out: *mut u8) -> *mut u8;
}
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("secure OS randomness unavailable");
    b
}
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    unsafe {
        assert!(!SHA256(data.as_ptr(), data.len(), out.as_mut_ptr()).is_null());
    }
    out
}
pub fn uuid_v4() -> String {
    let mut b = random_bytes::<16>();
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub fn b64url_encode(data: &[u8]) -> String {
    let mut s = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            s.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    s
}
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().take_while(|c| *c != b'=') {
        acc = (acc << 6) | B64.iter().position(|b| *b == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for len in 0..40 {
            let x: Vec<u8> = (0..len).collect();
            assert_eq!(b64url_decode(&b64url_encode(&x)).unwrap(), x);
        }
    }
    #[test]
    fn pkce() {
        assert_eq!(
            b64url_encode(&sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
