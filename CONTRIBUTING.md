# Contributing to VinylPlayer

Thanks for helping improve VinylPlayer. Bug reports, accessibility feedback, documentation fixes, and focused code changes are welcome.

## Before opening an issue

- Search existing issues first.
- Include your Linux distribution, desktop session (Wayland or X11), GTK/Libadwaita versions, PipeWire version, and VinylPlayer version.
- For playback problems, include the audio format and relevant terminal output. Do not attach copyrighted audio or Last.fm credentials.

## Development setup

Install the dependencies listed in the README, then run:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
```

Tests that require a graphical session or external multimedia tools are ignored by default and should be run explicitly when the change touches those areas.

## Pull requests

- Keep each pull request focused on one problem.
- Add or update tests for behavior changes.
- Preserve English and Brazilian Portuguese localization for user-facing text.
- Keep blocking I/O, file parsing, and network requests off the GTK main thread.
- Explain the user-visible effect and how the change was tested.

By contributing, you agree that your contribution is licensed under GPL-3.0-or-later.
