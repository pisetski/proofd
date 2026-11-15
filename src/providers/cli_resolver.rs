use super::ProviderError;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn resolve_cli(configured_bin: Option<&str>, name: &str) -> Result<String, ProviderError> {
    if let Some(configured) = configured_bin.map(str::trim).filter(|s| !s.is_empty()) {
        return resolve_configured(&expand_tilde(configured));
    }
    if let Some(found) = find_on_path(name) {
        return Ok(found);
    }
    find_in_common_install_dirs(name).ok_or_else(|| {
        ProviderError::Transport(format!(
            "spawn {name}: not found on PATH ({}) nor in {} — \
             set `provider_bin` in config (e.g. `provider_bin = \"/opt/homebrew/bin/{name}\"`)",
            path_for_error(),
            common_install_dirs().join(", "),
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
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).display().to_string();
    }
    s.to_string()
}

fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

// The Services host runs with a minimal PATH that misses Homebrew and ~/.local/bin.
fn find_in_common_install_dirs(name: &str) -> Option<String> {
    common_install_dirs()
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

const HOME_INSTALL_DIRS: [&str; 8] = [
    ".cargo/bin",
    ".local/bin",
    ".opencode/bin",
    ".bun/bin",
    ".npm-global/bin",
    ".volta/bin",
    ".local/share/mise/shims",
    ".asdf/shims",
];
// nvm follows XDG_CONFIG_HOME when it is set, and a GUI app doesn't know whether it was.
const NVM_ROOTS_IN_HOME: [&str; 2] = [".nvm", ".config/nvm"];

pub(super) fn common_install_dirs() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
        "/bin",
    ]
    .map(PathBuf::from)
    .into();
    if let Some(home) = dirs::home_dir() {
        dirs.extend(install_dirs_under(&home));
    }
    dirs.iter().map(|dir| dir.display().to_string()).collect()
}

fn install_dirs_under(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = HOME_INSTALL_DIRS.iter().map(|sub| home.join(sub)).collect();
    dirs.extend(
        NVM_ROOTS_IN_HOME
            .iter()
            .filter_map(|root| newest_nvm_node_bin(&home.join(root))),
    );
    dirs
}

fn newest_nvm_node_bin(nvm_root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(nvm_root.join("versions/node"))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = node_version(&entry.file_name().to_string_lossy())?;
            Some((version, entry.path().join("bin")))
        })
        .filter(|(_, bin)| bin.is_dir())
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, bin)| bin)
}

fn node_version(dir_name: &str) -> Option<Vec<u64>> {
    dir_name
        .strip_prefix('v')?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

// A CLI installed with npm starts with `#!/usr/bin/env node`, and node sits beside it.
pub(super) fn path_for_child(program: &str) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let program_dir = Path::new(program).parent().map(Path::to_path_buf);
    let dirs: Vec<PathBuf> = program_dir
        .into_iter()
        .chain(std::env::split_paths(&inherited))
        .chain(common_install_dirs().into_iter().map(PathBuf::from))
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    std::env::join_paths(dirs).unwrap_or(inherited)
}

fn path_for_error() -> String {
    std::env::var("PATH").unwrap_or_else(|_| "<unset>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_configured_absolute_path() {
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
    fn resolve_configured_missing_errors_clearly() {
        let err = resolve_cli(Some("/nonexistent-xyz/proofd-opencode"), "opencode")
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider_bin"), "got: {err}");
    }

    #[test]
    fn resolve_configured_bare_name_missing_errors_clearly() {
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

    #[test]
    fn nvm_picks_the_newest_node_by_number_not_by_text() {
        let nvm = tempfile::tempdir().unwrap();
        for version in ["v9.11.2", "v25.2.1", "v10.0.0", "not-a-version"] {
            std::fs::create_dir_all(nvm.path().join("versions/node").join(version).join("bin"))
                .unwrap();
        }
        std::fs::create_dir_all(nvm.path().join("versions/node/v99.0.0")).unwrap();

        assert_eq!(
            newest_nvm_node_bin(nvm.path()),
            Some(nvm.path().join("versions/node/v25.2.1/bin"))
        );
        assert_eq!(newest_nvm_node_bin(&nvm.path().join("missing")), None);
    }

    #[test]
    fn child_path_starts_with_the_programs_directory_then_the_inherited_path_then_the_install_dirs()
    {
        let path = path_for_child("/nvm/versions/node/v25/bin/claude");
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();

        assert_eq!(dirs[0], Path::new("/nvm/versions/node/v25/bin"));
        let inherited: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        assert_eq!(dirs[1..=inherited.len()], inherited[..]);
        assert!(
            dirs.ends_with(
                &common_install_dirs()
                    .into_iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            )
        );
    }

    #[test]
    fn install_dirs_cover_npm_and_the_node_version_managers() {
        let home = tempfile::tempdir().unwrap();
        let nvm_bin = home.path().join(".config/nvm/versions/node/v25.2.1/bin");
        std::fs::create_dir_all(&nvm_bin).unwrap();

        let dirs = install_dirs_under(home.path());

        for expected in [
            ".npm-global/bin",
            ".volta/bin",
            ".local/share/mise/shims",
            ".asdf/shims",
            ".local/bin",
        ] {
            assert!(
                dirs.contains(&home.path().join(expected)),
                "missing {expected}"
            );
        }
        assert!(dirs.contains(&nvm_bin), "dirs: {dirs:?}");
    }
}
