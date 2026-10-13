#!/usr/bin/env bash
# Instalador portátil do Dish. Sem sudo: instala em ~/.local e no XDG_DATA_HOME.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
action=""

usage() {
    cat <<'EOF'
Uso: ./install.sh [install|uninstall]

Sem argumento, mostra um menu (Instalar / Desinstalar / Sair) quando há
terminal; sem terminal, instala, como antes.

  install     instala em ~/.local/bin e cria o atalho no menu de aplicativos
  uninstall   remove o Dish instalado por este pacote
  -h, --help  mostra esta ajuda
EOF
}

installed_path() {
    printf '%s' "$HOME/.local/bin/dish"
}

require_platform() {
    [[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || {
        echo 'Este pacote requer Linux x86_64.' >&2; exit 1;
    }
}

install_dish() {
    require_platform
    [[ -x "$root/bin/dish" ]] || { echo 'Binário do pacote não encontrado.' >&2; exit 1; }
    command -v python3 >/dev/null || { echo 'Python 3 é necessário para instalar o atalho.' >&2; exit 1; }
    local bin_dir="$HOME/.local/bin"
    local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
    [[ "$HOME" = /* && "$data_dir" = /* ]] || { echo 'HOME e XDG_DATA_HOME devem ser caminhos absolutos.' >&2; exit 1; }
    # Quote the executable according to the Desktop Entry specification, not shell syntax.
    local exec_path
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
}

uninstall_dish() {
    # A remoção vive em uninstall.sh para existir num só lugar; o pacote sempre
    # traz os dois scripts lado a lado.
    [[ -f "$root/uninstall.sh" ]] || { echo 'uninstall.sh não encontrado ao lado do install.sh.' >&2; exit 1; }
    exec bash "$root/uninstall.sh"
}

choose_action() {
    if [[ -e "$(installed_path)" ]]; then
        printf '\nDish já instalado em %s\n' "$(installed_path)"
    fi
    printf '\n  i  Instalar\n  u  Desinstalar\n  q  Sair\n\n'
    while :; do
        printf 'Escolha [i]: '
        local key=""
        IFS= read -r key || key=""
        case "$key" in
            ""|i|I|y|Y) action=install; return ;;
            u|U) action=uninstall; return ;;
            q|Q|n|N) echo 'Nada foi alterado.'; exit 0 ;;
            *) printf 'Escolha i, u ou q.\n' ;;
        esac
    done
}

case "${1:-}" in
    "") ;;
    install|--install|-i) action=install ;;
    uninstall|--uninstall|-u) action=uninstall ;;
    -h|--help|help) usage; exit 0 ;;
    *) printf 'Opção desconhecida: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
esac

if [[ -z "$action" ]]; then
    if [[ -t 0 ]]; then
        choose_action
    else
        action=install
        printf 'Sem terminal interativo; instalando. Use "%s uninstall" para remover.\n' "$0"
    fi
fi

case "$action" in
    install) install_dish ;;
    uninstall) uninstall_dish ;;
esac
