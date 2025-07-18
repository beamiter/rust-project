{
  description = "A development environment for a Rust GTK4 application";

  # --- 输入 ---
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  # --- 输出 ---
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        # --- 开发环境 ---
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy

            pkgs.gtk4
            pkgs.glib
            pkgs.pkg-config

            pkgs.glade
            pkgs.cambalache

            pkgs.vte
            pkgs.vte-gtk4
            pkgs.libsoup_3
            pkgs.webkitgtk_4_1
            pkgs.alsa-lib
            pkgs.xdotool
            pkgs.libadwaita
            pkgs.librsvg
            pkgs.libcanberra-gtk3

            # pkgs.appimageTools
            pkgs.patchelf
            pkgs.fuse
            pkgs.fakeroot
          ];

          # shellHook 内容不变
          shellHook = ''
            export GSETTINGS_SCHEMA_DIR="${pkgs.gtk4}/share/gsettings-schemas/:${pkgs.glib}/share/gsettings-schemas/"
            export RUST_BACKTRACE=1
            echo "Rust GTK4 development and bundling environment is ready."
          '';
        };
      }
    );
}
