#[tokio::main]
async fn main() {
    umohobot::logging::init_tracing();

    if let Err(err) = umohobot::runtime::run().await {
        tracing::error!(error = %err, "fatal runtime error");
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
