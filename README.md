A macOS service that proofreads selected text in place using an LLM CLI.

![Selected text corrected in place](docs/demo.gif)

Select text, then right-click › Services › Proofread Selection.
Or bind it to your own shortcut, like ⌘⌃⌥P.

It fixes errors only, and keeps your tone, language, and Markdown.
Uses your existing `claude` or `opencode` login, so no API key is needed.
Selected text is sent to your configured provider.

## Install

Requires Xcode Command Line Tools, [Rust](https://rustup.rs) 1.75 or newer, and
`claude` or `opencode`.

```sh
git clone https://github.com/pisetski/proofd && cd proofd
make install
```

This installs `proofd.app` to `~/Applications` and links the `proofd` command
into `~/.local/bin`, which must be on your `PATH` to use it from the terminal.

Then enable **Proofread Selection** in System Settings › Keyboard › Keyboard
Shortcuts › Services › Text, and assign your shortcut.

For copy-first use, assign a separate shortcut to **Proofread Clipboard** under
Services. Press ⌘C, invoke that shortcut, then press ⌘V when the popup says **Copied**.
This action doesn't require the app to provide selected text.

## Terminal

```sh
echo "helo world" | proofd
```

## Configure

Optional. To change the provider (`claude`, `opencode`, or a local
OpenAI-compatible server), model, or timeout:

```sh
mkdir -p ~/.config/proofd
cp config.example.toml ~/.config/proofd/config.toml
```

To change the prompt, copy `skills/proofread.md` to
`~/.config/proofd/proofread.md` and edit it.

### Result popup

To preview and copy the result instead of replacing text, add to your config:

```toml
show_result_popup = true
```

Use the same shortcut, wait for **Copied**, then press ⌘V. Click outside to dismiss.
Requires the app to support macOS Services; no additional permissions needed.

## Uninstall

```sh
make uninstall
```

## License

MIT or Apache-2.0.
