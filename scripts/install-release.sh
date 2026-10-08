#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
    echo 'Este pacote requer Linux x86_64.' >&2; exit 1;
}
[[ -x "$root/bin/dish" ]] || { echo 'Binário do pacote não encontrado.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 é necessário para instalar o atalho.' >&2; exit 1; }
bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
[[ "$HOME" = /* && "$data_dir" = /* ]] || { echo 'HOME e XDG_DATA_HOME devem ser caminhos absolutos.' >&2; exit 1; }
# Quote the executable according to the Desktop Entry specification, not shell syntax.
exec_path=$(printf '%s' "$bin_dir/dish" | python3 -c 'import sys; s=sys.stdin.read(); print(s.replace("\\", "\\\\\\\\").replace("\"", "\\\"").replace("`", "\\`").replace("$", "\\$").replace("%", "%%"), end="")')
install -Dm755 "$root/bin/dish" "$bin_dir/dish"
install -Dm644 "$root/share/icons/dish.png" "$data_dir/icons/hicolor/512x512/apps/dish.png"
mkdir -p "$data_dir/applications" "$data_dir/dish/licenses"
cp -R "$root/share/licenses/." "$data_dir/dish/licenses/"
printf '[Desktop Entry]\nType=Application\nName=Dish\nComment=Desktop interface for the Pi coding agent\nExec="%s"\nIcon=dish\nTerminal=false\nCategories=Development;\n' "$exec_path" > "$data_dir/applications/dish.desktop"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data_dir/applications" || true; fi
if command -v gtk-update-icon-cache >/dev/null && [[ -f "$data_dir/icons/hicolor/index.theme" ]]; then
    gtk-update-icon-cache -f -t "$data_dir/icons/hicolor" || true
fi
printf 'Instalado: %s\nAbra Dish pelo menu de aplicativos. O Pi pode ser configurado na primeira abertura.\n' "$bin_dir/dish"
