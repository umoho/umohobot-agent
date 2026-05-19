#[tokio::main]
async fn main() {
    agent::logging::init_tracing();

    if let Err(err) = tools_telegram::run().await {
        tracing::error!(error = %err, "fatal telegram runtime error");
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
