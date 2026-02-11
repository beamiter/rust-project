# jterm4

A simple GTK4 + VTE terminal with tab support.

## Tabs

When creating a new tab, `jterm4` attempts to inherit the current tab's working directory (via VTE's current directory URI). If unavailable, it falls back to `$HOME`.

Each tab also includes a close button in the tab header.

On exit, `jterm4` saves the current tab list (and active tab) and restores them on next start. The state is stored under your XDG config directory (typically `~/.config/jterm4/tabs.state`).

## Configuration (environment variables)

- `JTERM4_LOG`: log level (`off|error|warn|info|debug|trace`). Falls back to `RUST_LOG`. Default: `warn`.
- `JTERM4_FONT`: Pango font description string. Default: `SauceCodePro Nerd Font Regular 12`.
- `JTERM4_FONT_SCALE`: default font scale (0.1-10.0). Default: `1.0`.
- `JTERM4_SCROLLBACK`: scrollback line count (>=0). Default: `5000`.
- `JTERM4_OPACITY`: initial window opacity (0.01-1.0). Default: `0.95`.
- `JTERM4_FG`, `JTERM4_BG`, `JTERM4_CURSOR`, `JTERM4_CURSOR_FG`: colors accepted by GTK (`#RRGGBB` etc).

## Shell selection

On startup, each terminal chooses a shell like this:

- Prefer `fish` when available.
- If `fish` has a working `bass`, run `bass "source ~/.bashrc"` once before showing the prompt (so bash-style environment changes from `.bashrc` are applied).
- If `fish` is not available, fall back to `bash -l`.

Notes:

- This project does not need to edit `~/.config/fish/config.fish`.
- `bass` is expected to import environment variables; importing bash aliases into fish is intentionally avoided because many bash aliases contain bash-only syntax.

## Installing bass (fish)

If you want `.bashrc` environment imported into fish, install `bass`:

- With fisher: `fisher install edc/bass`

(Any other method that provides a `bass` function in fish also works.)
