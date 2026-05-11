fn main() {
    let config = seriousum_config::default_config();
    println!(
        "seriousum-config: agent={}, cluster={}, mtu={}",
        config.agent.name, config.agent.cluster_name, config.network.mtu
    );
}
