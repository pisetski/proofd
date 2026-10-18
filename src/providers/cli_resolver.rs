use super::ProviderError;
use std::path::Path;

// The Services host runs with a minimal PATH, so bare names miss Homebrew and ~/.local/bin installs.
pub fn resolve_cli(explicit: Option<&str>, name: &str) -> Result<String, ProviderError> {
    if let Some(configured) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return resolve_configured(&expand_tilde(configured));
    }
    if let Some(found) = find_on_path(name) {
        return Ok(found);
    }
    find_in_fallback_dirs(name).ok_or_else(|| {
        ProviderError::Transport(format!(
            "spawn {name}: not found on PATH ({}) nor in {} — \
             set `provider_bin` in config (e.g. `provider_bin = \"/opt/homebrew/bin/{name}\"`)",
            path_for_error(),
            fallback_dirs().join(", "),
        ))
    })
}

fn resolve_configured(expanded: &str) -> Result<String, ProviderError> {
    if expanded.contains('/') {
        if Path::new(expanded).is_file() {
            return Ok(expanded.to_string());
        }
        return Err(ProviderError::Transport(format!(
            "provider_bin {expanded:?} not found; check `provider_bin` in config"
        )));
    }
    find_on_path(expanded).ok_or_else(|| {
        ProviderError::Transport(format!(
            "provider_bin {expanded:?} not found on PATH ({})",
            path_for_error()
        ))
    })
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    s.to_string()
}

fn find_on_path(name: &str) -> Option<String> {
    if name.contains('/') {
        let expanded = expand_tilde(name);
        return Path::new(&expanded)
            .is_file()
            .then(|| Path::new(&expanded).display().to_string());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

fn find_in_fallback_dirs(name: &str) -> Option<String> {
    fallback_dirs()
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

fn fallback_dirs() -> Vec<String> {
    let mut dirs = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/opt/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
    ];
    if let Some(home) = dirs::home_dir() {
        for sub in [".cargo/bin", ".local/bin", ".opencode/bin", ".bun/bin"] {
            dirs.push(home.join(sub).display().to_string());
        }
    }
    dirs
}

fn path_for_error() -> String {
    std::env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_explicit_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("my-opencode");
        std::fs::write(&bin, "#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let got = resolve_cli(Some(bin.to_str().unwrap()), "opencode").unwrap();
        assert_eq!(got, bin.display().to_string());
    }

    #[test]
    fn resolve_explicit_missing_errors_clearly() {
        let err = resolve_cli(Some("/nonexistent-xyz/proofd-opencode"), "opencode")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
    }

    #[test]
    fn resolve_explicit_bare_name_missing_errors_clearly() {
        let err = resolve_cli(Some("definitely-not-a-real-binary-xyz"), "opencode")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
        assert!(err.contains("not found on PATH"), "got: {err}");
    }

    #[test]
    fn resolve_missing_binary_mentions_provider_bin() {
        let err = resolve_cli(None, "definitely-not-a-real-binary-xyz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
    }
}
