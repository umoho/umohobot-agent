#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Image,
    Audio,
}

impl std::str::FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "image" => Ok(Capability::Image),
            "audio" => Ok(Capability::Audio),
            _ => Err(format!("Unknown capability: {s}")),
        }
    }
}
