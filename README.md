# proofd

Proofreads selected text from the macOS Services menu. Select text in any app,
then choose Services > Proofread Selection (or assign it a keyboard shortcut in
System Settings > Keyboard > Keyboard Shortcuts > Services). The selection is
replaced with the revised text.

No Accessibility or Input Monitoring permission is needed: the system hands the
selection to the service and takes the result back.

## Install

Needs Xcode Command Line Tools and Rust.

```sh
make install
```

This builds `~/Applications/proofd.app`, links its `proofd` CLI into
`~/.local/bin` and registers the service. On first install, System Settings
opens at Keyboard; enable "Proofread Selection" under Text in Keyboard
Shortcuts > Services. Re-run to update; `make uninstall` removes it.

No config is needed: without one, proofd uses `claude`, or `opencode` when
only that is installed.

## CLI

`proofd polish` reads text from stdin and writes the revised text to stdout.
Logs go to stderr. It exits non-zero on empty input or provider failure.

```sh
echo "helo world" | proofd polish
```

## Logs

`make logs` follows `~/Library/Logs/proofd-service.log` and the Services host's
system log. `make test-service` runs an end-to-end check of the installed service.

## Config

TOML at `~/.config/proofd/config.toml` (also `$XDG_CONFIG_HOME/proofd/config.toml`;
legacy macOS location `~/Library/Application Support/proofd/config.toml` still
works as fallback, first existing file wins). See `config.example.toml`. Missing
file means defaults (see Install), not crash; invalid file returns a clear error and
makes `proofd polish` fail.

Defaults: provider `claude`, model `haiku`, timeout `15`s,
`max_input_chars = 12000`.

Optional skill override: `~/.config/proofd/proofread.md` (same lookup order).

## Providers

- `claude`: `claude -p --model <model> --output-format text`, text via stdin,
  system prompt via `--system-prompt`. Runs isolated: empty cwd under the user
  cache dir, no tools, no skills, no settings sources, no session persistence.
  Verify flags with `claude --help`; avoid `--bare` (disables OAuth login).
- `opencode`: `opencode run --model <provider/model> --format default`,
  prompt via stdin. `--model` needs the `provider/model` form
  (e.g. `opencode/mimo-v2.6-flash-free`); bare names fail server-side.
  Verify with `opencode run --help`; flags drift.
- `openai-compat`: `POST {base_url}/chat/completions`.

Timeout enforced by caller; empty output is an error and is never pasted.

### PATH

The service host runs with a minimal `PATH`, so `proofd` searches `PATH` plus
`/opt/homebrew/bin`, `/usr/local/bin`, `~/.cargo/bin`, `~/.local/bin`,
`~/.opencode/bin`, `~/.bun/bin` for the provider CLI and reports the searched
`PATH` when it is missing. If yours lives elsewhere, set `provider_bin`:

```toml
provider = "opencode"
model = "opencode/mimo-v2.6-flash-free"
provider_bin = "/opt/homebrew/bin/opencode"
```

## Limitations

- macOS only.
- Plain text only; rich text formatting in the selection is not preserved.
- Selections above `max_input_chars` are refused.

## QA

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release
```

Then `make test-service` against an installed build.

## License

Dual MIT/Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
