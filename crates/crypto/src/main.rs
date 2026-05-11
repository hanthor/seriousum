fn main() {
    let fingerprint = seriousum_crypto::Fingerprint::sha256(b"seriousum");
    println!("seriousum-crypto: {fingerprint}");
}
