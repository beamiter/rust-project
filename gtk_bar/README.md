# gtk_bar

This crate depends on GTK4 from Nixpkgs (newer GTK), so building outside the Nix dev shell may fail.

## Build

From the workspace root:

- Build (Wayland/X11 auto by GTK runtime):
  - `nix develop -c cargo build -p gtk_bar`
- Run:
  - `nix develop -c cargo run -p gtk_bar -- <shared_path>`

## Optional X11 helpers

Enable the `x11` feature only if you need the X11 window-move/flush helper code:

- `nix develop -c cargo build -p gtk_bar --features x11`
