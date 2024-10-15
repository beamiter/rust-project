{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = [
    pkgs.rustc          # Rust 编译器
    pkgs.cargo          # Cargo，Rust 的包管理器和构建工具
    pkgs.rustfmt        # Rust 代码格式化工具
    pkgs.clippy         # Rust 静态分析工具，用于捕获常见错误
    pkgs.gtk4
    pkgs.gdk4
    pkgs.pkg-config
    pkgs.vte
    pkgs.vte-gtk4
    pkgs.glib         # GTK的辅助库
    # 如果需要，还可以添加其他依赖，例如：
    # pkgs.gtk4.dev
    # pkgs.gobject-introspection
    # pkgs.vala 或 pkgs.rustPlatform.rustc (取决于使用何种编程语言)
  ];

  # 设置必要的环境变量
  shellHook = ''
    export GSETTINGS_SCHEMA_DIR=$GSETTINGS_SCHEMA_DIR${pkgs.glib.out}/share/gsettings-schemas/${pkgs.glib.name}
    export PKG_CONFIG_PATH="$PKG_CONFIG_PATH:${pkgs.lib.makeLibraryPath [ pkgs.vte ]}/lib/pkgconfig"
    export RUST_BACKTRACE=1
  '';
}
