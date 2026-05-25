pub struct TimerConfig {
    pub max_per_thread: usize,
    pub min_delay_secs: u64,
    pub max_delay_secs: u64,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            max_per_thread: 5,
            min_delay_secs: 10,
            max_delay_secs: 604800,
        }
    }
}
