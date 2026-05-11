fn main() {
    let report = seriousum_cni::scaffold();
    match serde_json::to_string_pretty(&report) {
        Ok(payload) => println!("{payload}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
