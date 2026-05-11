use seriousum_policy::PolicyRepository;

#[tokio::main]
async fn main() {
    let repository = PolicyRepository::new();
    
    println!("✓ Policy engine initialized");
    println!("  Ingress rules: {}", repository.ingress_rule_count());
    println!("  Egress rules: {}", repository.egress_rule_count());
    println!("Ready to enforce network policies");
}
