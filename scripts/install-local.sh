#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo build --release --locked
bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm755 target/release/dish "$bin_dir/dish"
install -Dm644 assets/dish-icon.png "$data_dir/icons/hicolor/512x512/apps/dish.png"
mkdir -p "$data_dir/applications"
# Desktop launchers do not necessarily inherit the terminal's PATH.
pi_bin="${DISH_PI_BIN:-$(command -v pi || true)}"
if [[ -z "$pi_bin" ]]; then
    echo 'Pi não encontrado: instale-o ou defina DISH_PI_BIN antes de executar este script.' >&2
    exit 1
fi
# Escape characters with special meaning inside quoted desktop Exec arguments.
escape_exec() {
    printf '%s' "$1" | python3 -c 'import sys; s=sys.stdin.read(); print(s.replace("\\", "\\\\\\\\").replace("\"", "\\\"").replace("`", "\\`").replace("$", "\\$").replace("%", "%%"), end="")'
}
printf '[Desktop Entry]\nType=Application\nName=Dish\nComment=Interface desktop para o Pi coding agent\nExec=env "DISH_PI_BIN=%s" "%s"\nIcon=dish\nTerminal=false\nCategories=Development;\n' "$(escape_exec "$pi_bin")" "$(escape_exec "$bin_dir/dish")" > "$data_dir/applications/dish.desktop"
echo "Instalado: $bin_dir/dish"
echo 'Abra Dish pelo menu de aplicativos ou execute ~/.local/bin/dish /caminho/do/projeto'
