fn main() {
    if let Err(err) = umohobot::app::run() {
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}
