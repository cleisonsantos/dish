#!/usr/bin/env bash
set -euo pipefail
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
[[ "$HOME" = /* && "$data_dir" = /* ]] || { echo 'HOME e XDG_DATA_HOME devem ser caminhos absolutos.' >&2; exit 1; }
rm -f -- "$HOME/.local/bin/dish" "$data_dir/applications/dish.desktop" "$data_dir/icons/hicolor/512x512/apps/dish.png"
rm -rf -- "$data_dir/dish/licenses"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data_dir/applications" || true; fi
echo 'Dish removido. Preferências, instalação do Pi, projetos e sessões foram preservados.'
