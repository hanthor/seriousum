#[tokio::main]
async fn main() {
    let ipam = seriousum_ipam::Ipam::new();
    println!("IPAM initialized");

    // Create a default pool
    let pool = seriousum_ipam::Pool::default();
    let cidr: ipnet::IpNet = "10.0.0.0/24".parse().unwrap();
    ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();

    println!("Pool added: {}", pool);
}
