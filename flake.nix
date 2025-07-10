{
  description = "A development environment for a Rust GTK4 application";

  # --- 输入 ---
  # 定义此 Flake 的依赖项，例如 Nixpkgs
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  # --- 输出 ---
  # 定义此 Flake 提供的包、开发环境等
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # 为当前系统获取 pkgs
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        # --- 开发环境 ---
        # 通过 `nix develop` 命令进入
        devShells.default = pkgs.mkShell {

          # 相当于 shell.nix 中的 buildInputs
          packages = [
            # Rust 工具链
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy

            # GTK4 和相关系统库
            pkgs.gtk4
            pkgs.glib
            pkgs.pkg-config # 用于让 Rust 的 build scripts 找到系统库
            # pkgs.vte 包含在 vte-gtk4 中，但为了明确性可以保留
            pkgs.vte
            pkgs.vte-gtk4
            # 为 `soup3-sys` crate 提供 `libsoup-3.0` 系统库
            pkgs.libsoup_3

            # 为 `javascriptcore-rs-sys` crate 提供 `javascriptcoregtk-4.1`
            pkgs.webkitgtk_4_1

            # 为 `alsa-sys` crate 提供 `alsa` (libasound) 系统库
            pkgs.alsa-lib

            pkgs.xdotool

            pkgs.libadwaita
            pkgs.librsvg
          ];

          # 相当于 shell.nix 中的 shellHook
          # 注意：PKG_CONFIG_PATH 通常由 mkShell 自动处理，无需手动设置
          shellHook = ''
            # 在 GTK 应用中，设置 GSettings Schema 路径是一个好习惯
            # GSETTINGS_SCHEMA_DIR 变量确保应用能找到其设置定义
            export GSETTINGS_SCHEMA_DIR="${pkgs.gtk4}/share/gsettings-schemas/:${pkgs.glib}/share/gsettings-schemas/"

            # 方便调试 Rust 程序
            export RUST_BACKTRACE=1

            echo "Rust GTK4 development environment is ready."
          '';
        };
      }
    );
}
