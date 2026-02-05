#!/bin/bash
#
# Test script to verify IME environment variables are set correctly
#

echo "=== IME Environment Variables Check ==="
echo ""

# Check which IME is running
echo "Running IME processes:"
ps aux | grep -E "fcitx|ibus" | grep -v grep | awk '{print $11}' | sort -u
echo ""

# Check environment variables
echo "Current environment variables:"
echo "  GTK_IM_MODULE: ${GTK_IM_MODULE:-NOT SET}"
echo "  XMODIFIERS: ${XMODIFIERS:-NOT SET}"
echo "  QT_IM_MODULE: ${QT_IM_MODULE:-NOT SET}"
echo ""

# Check if they match
if ps aux | grep -v grep | grep -q fcitx; then
    echo "✓ fcitx is running"
    if [ "$GTK_IM_MODULE" = "fcitx" ]; then
        echo "✓ GTK_IM_MODULE is correctly set to fcitx"
    else
        echo "✗ GTK_IM_MODULE should be 'fcitx' but is '${GTK_IM_MODULE:-NOT SET}'"
        echo "  Run: source ~/.bashrc (for bash) or restart your shell"
    fi
elif ps aux | grep -v grep | grep -q ibus; then
    echo "✓ ibus is running"
    if [ "$GTK_IM_MODULE" = "ibus" ]; then
        echo "✓ GTK_IM_MODULE is correctly set to ibus"
    else
        echo "✗ GTK_IM_MODULE should be 'ibus' but is '${GTK_IM_MODULE:-NOT SET}'"
        echo "  Run: source ~/.bashrc (for bash) or restart your shell"
    fi
else
    echo "✗ No IME (fcitx or ibus) is running"
    echo "  Please start your input method first"
fi

echo ""
echo "=== Next Steps ==="
echo "1. If variables are not set, run: source ~/.bashrc"
echo "2. Or restart your terminal/shell"
echo "3. Run this script again to verify"
echo "4. Then run jterm4: cargo run"
echo ""
