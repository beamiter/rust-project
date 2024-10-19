#!/bin/sh

xrdb merge ~/.Xresources
xbacklight -set 10 &
# feh --bg-fill ~/Pictures/wall/gruv.png &
# xset r rate 200 50 &
picom &

bash /home/yj/.config/jwm/scripts/bar.sh &
while type jwm >/dev/null; do jwm && continue || break; done
