# proofd

Standalone Rust daemon, macOS-first. Global `Ctrl-Option-P` polishes selected
text, falling back to focused text box contents via simulated `Cmd+A`,
`Cmd+C`, `Cmd+V`. Replacement is silent on success and supports native
single `Cmd+Z` undo.

- Simulated paste only. Never mutates app text with `AXSetValue`.
- Silent success; error notification only, no sound or beep.
- Single `Cmd+V` keeps native undo.

## Install

```sh
cargo install --path .
# or
cargo build --release
```

Requires macOS Accessibility permission for simulated key events; macOS may
also request Input Monitoring.

## Config

TOML at `~/.config/proofd/config.toml` (also `$XDG_CONFIG_HOME/proofd/config.toml`;
legacy macOS location `~/Library/Application Support/proofd/config.toml` still
works as fallback, first existing file wins). See `config.example.toml`. Missing
file means defaults, not crash; invalid file returns a clear error and
prevents daemon start.

Defaults: provider `claude`, model `haiku`, timeout `15`s,
hotkey `Ctrl-Alt-P`, `max_input_chars = 12000`.

Optional skill override: `~/.config/proofd/proofread.md` (same lookup order).

## Autostart (LaunchAgent)

Edit `contrib/com.proofd.plist` if `proofd` is not at
`~/.cargo/bin/proofd`, copy to `~/Library/LaunchAgents/com.proofd.plist`:

```sh
cp contrib/com.proofd.plist ~/Library/LaunchAgents/com.proofd.plist
launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.proofd.plist
# legacy fallback: launchctl load ~/Library/LaunchAgents/com.proofd.plist
```

## Providers

- `claude`: `claude -p --model <model> --output-format text`, prompt via stdin.
  Verify flags with `claude --help` before adding persistence/tool flags.
- `opencode`: `opencode run --model <model> --format default`.
  Verify with `opencode --help`; flags drift.
- `openai-compat`: `POST {base_url}/chat/completions`.

Timeout enforced by caller; empty output is an error and is never pasted.

## Limitations

- Plaintext clipboard MVP only; rich clipboard contents may not be preserved.
- Fallback `Cmd+A` depends on focused text-field behavior; aborts above
  `max_input_chars`.
- Paste aborts if front app PID changed between trigger and paste.
- Linux: unsupported-platform error (MVP is macOS-only).

## QA

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release
```

Manual matrix: TextEdit, Telegram, Slack, Teams; selected replacement,
no-selection fallback, multiline, Markdown/code block, ~2k chars,
offline failure keeps original, plaintext clipboard restored, single
`Cmd+Z` restores original, focus change aborts, success silent, error notifies.

## License

Dual MIT/Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
