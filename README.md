# proofd

Proofread selected text in any macOS app.

## Install

Requires Xcode Command Line Tools, Rust, and `claude` or `opencode`.

```sh
make install
```

Then enable **Proofread Selection** in System Settings › Keyboard › Keyboard
Shortcuts › Services › Text, and optionally assign a shortcut.

## Use

Select text, then choose Services › Proofread Selection. The selection is
replaced with the corrected text.

From the terminal:

```sh
echo "helo world" | proofd polish
```

## Configure

Optional. Copy `config.example.toml` to `~/.config/proofd/config.toml`.
To customize the prompt, add `~/.config/proofd/proofread.md`.

## Uninstall

```sh
make uninstall
```

## License

MIT or Apache-2.0.
