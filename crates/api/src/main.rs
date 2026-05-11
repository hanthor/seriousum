fn main() {
    let version = seriousum_api::VersionInfo::current();
    println!("seriousum-api {} (core {})", version.contract, version.core);
}
