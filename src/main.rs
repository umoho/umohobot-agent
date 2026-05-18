#[tokio::main]
async fn main() {
    if let Err(err) = umohobot::runtime::run().await {
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
