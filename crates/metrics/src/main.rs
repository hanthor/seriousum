fn main() {
    let opts = seriousum_metrics::MetricOpts::new("test_metric");
    println!("Metrics module ready: {}", opts.fq_name());
}
