fn main() {
    if let Err(error) = seriousum_clustermesh::run().map(|output| println!("{output}")) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
