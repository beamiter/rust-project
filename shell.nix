# 方法二：使用系统配置的 nixos-unstable 频道
# 这种写法更简洁，但可复现性稍差
{ pkgs ? import <nixos-unstable> {} }:

pkgs.mkShell {
  # buildInputs 和 shellHook 的内容保持不变
  buildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    clippy
    # ... 其他所有包 ...
    gtk4
    glib
    pkg-config
    vte
    vte-gtk4
    libsoup_3
    webkitgtk_4_1
    alsa-lib
    xdotool
    libadwaita
    librsvg
  ];

  shellHook = ''
    export GSETTINGS_SCHEMA_DIR="${pkgs.gtk4}/share/gsettings-schemas/:${pkgs.glib}/share/gsettings-schemas/"
    export RUST_BACKTRACE=1
    echo "Rust GTK4 development environment (from unstable channel) is ready."
  '';
}
