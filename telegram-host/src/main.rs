#[tokio::main]
async fn main() {
    agent::logging::init_tracing();

    if let Err(err) = telegram_host::runtime::run().await {
        tracing::error!(error = %err, "fatal runtime error");
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
