#!/bin/bash
#
# Launcher script for jterm4 with fcitx IME support
# This sets the correct environment variables for fcitx input method
#

export GTK_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export QT_IM_MODULE=fcitx

# Run jterm4
exec cargo run --release
