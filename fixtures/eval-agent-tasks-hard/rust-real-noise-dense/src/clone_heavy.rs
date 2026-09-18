//! Noise: heavy Clone/Debug derive style code — name-adjacent, no Encode edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub retries: u32,
    pub label: String,
    pub flags: Vec<u8>,
}

impl Config {
    pub fn clone_tight(&self) -> Self {
        self.clone()
    }
}

#[derive(Clone)]
pub struct Session {
    pub config: Config,
}

impl Session {
    pub fn reopen(&self) -> Session {
        Session {
            config: self.config.clone(),
        }
    }
}

// Unrelated helper name that greps may confuse with encode work.
pub fn encode_name_for_clone(c: &Config) -> String {
    format!("clone-{}", c.label)
}
