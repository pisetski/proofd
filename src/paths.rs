use std::path::PathBuf;

const APP_DIR: &str = "proofd";

pub fn candidate_paths(filename: &str) -> Vec<PathBuf> {
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| !dir.as_os_str().is_empty());
    let dot_config = dirs::home_dir().map(|home| home.join(".config"));
    let platform_config = dirs::config_dir();

    let mut unique: Vec<PathBuf> = Vec::new();
    for dir in [xdg_config_home, dot_config, platform_config]
        .into_iter()
        .flatten()
    {
        let path = dir.join(APP_DIR).join(filename);
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

pub fn existing_or_preferred_path(filename: &str) -> Option<PathBuf> {
    let candidates = candidate_paths(filename);
    candidates
        .iter()
        .find(|path| path.exists())
        .or_else(|| candidates.first())
        .cloned()
}
