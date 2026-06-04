use std::{
    collections::HashMap,
    path::PathBuf,
    time::Instant,
};

pub struct ServerState {
    pub net_previous: Option<(u64, u64, Instant)>,
    pub dir_aliases: HashMap<PathBuf, String>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            net_previous: None,
            dir_aliases: load_dir_aliases(),
        }
    }
}

fn load_dir_aliases() -> HashMap<PathBuf, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(home).join(".yrl/lib/dir-aliases");
    let mut map = HashMap::new();

    let Ok(contents) = std::fs::read_to_string(&path) else {
        return map;
    };

    for line in contents.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(PathBuf::from(key.trim()), value.trim().to_string());
        }
    }
    map
}
