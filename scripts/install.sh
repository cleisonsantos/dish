#!/bin/sh
# Instalador de uma linha do Dish: baixa o .tar.gz portátil da release, confere
# o SHA-256 e delega a instalação ao install.sh do próprio pacote.
set -eu

action=install
assume_yes=0
no_desktop=0
prefix=
requested_tag=

usage() {
    cat <<'EOF'
Uso: install.sh [opções]

Baixa o Dish portátil (Linux x86_64) do GitHub Releases e instala sem sudo em
~/.local/bin/dish, com o atalho e as licenças em ${XDG_DATA_HOME:-~/.local/share}.
Sem terminal, segue sem pedir confirmação.

  --version <tag>   instala uma versão específica (v0.1.0-beta.1 ou 0.1.0-beta.1)
  --prefix <dir>    instala sob <dir> em vez de $HOME: o instalador do pacote roda
                    com HOME=<dir> e XDG_DATA_HOME=<dir>/.local/share, então o
                    binário fica em <dir>/.local/bin/dish
  --no-desktop      não deixa o atalho em applications/dish.desktop
  -y, --yes         não pede confirmação
  --uninstall       remove a instalação existente
  -h, --help        mostra esta ajuda
EOF
}

valid_tag() {
    case "$1" in
        v*) version=${1#v} ;;
        *) return 1 ;;
    esac
    case "$version" in
        ''|*[!0-9A-Za-z.-]*) return 1 ;;
    esac
    core=${version%%-*}
    case "$core" in
        *.*.*) ;;
        *) return 1 ;;
    esac
    major=${core%%.*}
    rest=${core#*.}
    minor=${rest%%.*}
    patch=${rest#*.}
    for part in "$major" "$minor" "$patch"; do
        case "$part" in
            ''|*[!0-9]*|0?*) return 1 ;;
        esac
    done
    if [ "$core" != "$version" ]; then
        prerelease=${version#"$core"-}
        case "$prerelease" in
            ''|*[!0-9A-Za-z.-]*) return 1 ;;
        esac
    fi
    return 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || { printf 'Falta o valor de --version.\n\n' >&2; usage >&2; exit 2; }
            requested_tag=$2
            shift 2
            ;;
        --prefix)
            [ $# -ge 2 ] || { printf 'Falta o valor de --prefix.\n\n' >&2; usage >&2; exit 2; }
            prefix=$2
            shift 2
            ;;
        --no-desktop) no_desktop=1; shift ;;
        --yes|-y) assume_yes=1; shift ;;
        --uninstall) action=uninstall; shift ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'Opção desconhecida: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

if [ "$(id -u)" = 0 ]; then
    printf 'Dish é instalado por usuário em ~/.local e nunca precisa de sudo.\nExecute como seu usuário normal, sem sudo.\n' >&2
    exit 1
fi

system=$(uname -s)
machine=$(uname -m)
if [ "$system" != Linux ] || [ "$machine" != x86_64 ]; then
    printf 'Dish via este instalador requer Linux x86_64; detectado: %s %s. Veja os outros pacotes em https://github.com/cleisonsantos/dish/releases\n' "$system" "$machine" >&2
    exit 1
fi

missing=0
for command_name in curl tar sha256sum bash python3; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        printf 'Comando obrigatório não encontrado: %s\n' "$command_name" >&2
        missing=1
    fi
done
if [ "$missing" -ne 0 ]; then
    exit 1
fi

if [ -n "$prefix" ]; then
    case "$prefix" in
        /*) ;;
        *) printf '--prefix precisa de um caminho absoluto (começando com /).\n' >&2; exit 2 ;;
    esac
fi

if [ -n "$prefix" ]; then
    child_home=$prefix
    child_data=$prefix/.local/share
else
    child_home=${HOME:?HOME não está definido}
    child_data=${XDG_DATA_HOME:-$HOME/.local/share}
fi
bin_dir=$child_home/.local/bin

api_url=${DISH_INSTALL_API_URL:-https://api.github.com/repos/cleisonsantos/dish/releases?per_page=10}
base_url=${DISH_INSTALL_BASE_URL:-https://github.com/cleisonsantos/dish/releases/download}
base_url=${base_url%/}

if [ -n "$requested_tag" ]; then
    case "$requested_tag" in
        v*) tag=$requested_tag ;;
        *) tag=v$requested_tag ;;
    esac
else
    if ! releases=$(curl -fsSL --proto '=https,file' --tlsv1.2 "$api_url"); then
        printf 'Falha ao consultar %s\n' "$api_url" >&2
        exit 1
    fi
    tag=$(printf '%s\n' "$releases" | sed -n '/"tag_name"/{s/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p;q;}')
fi

if ! valid_tag "$tag"; then
    printf 'Versão inválida: %s\n' "$tag" >&2
    exit 2
fi
version=${tag#v}
asset=dish-$version-linux-x86_64.tar.gz
archive_url=$base_url/$tag/$asset
checksum_url=$archive_url.sha256

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

download() {
    if ! curl -fsSL --proto '=https,file' --tlsv1.2 -o "$2" "$1"; then
        printf 'Falha ao baixar %s\n' "$1" >&2
        exit 1
    fi
}

download "$archive_url" "$tmp/$asset"
download "$checksum_url" "$tmp/$asset.sha256"

# Por enquanto a verificação é só de checksum: o CI ainda não publica um
# manifesto assinado (latest.json/minisig), então não há assinatura a conferir.
expected_hash=
expected_name=
extra=
read -r expected_hash expected_name extra < "$tmp/$asset.sha256" || true
checksum_ok=1
case "$expected_hash" in
    *[!0-9a-f]*|'') checksum_ok=0 ;;
esac
if [ "${#expected_hash}" -ne 64 ]; then
    checksum_ok=0
fi
case "$expected_name" in
    "$asset"|"*$asset") ;;
    *) checksum_ok=0 ;;
esac
if [ -n "$extra" ]; then
    checksum_ok=0
fi
if [ "$checksum_ok" -eq 1 ]; then
    actual=$(sha256sum "$tmp/$asset")
    actual=${actual%% *}
    if [ "$actual" != "$expected_hash" ]; then
        checksum_ok=0
    fi
fi
if [ "$checksum_ok" -ne 1 ]; then
    printf 'Checksum não confere para %s; nada foi instalado.\n' "$asset" >&2
    exit 1
fi

mkdir "$tmp/pkg"
if ! tar -xzf "$tmp/$asset" -C "$tmp/pkg" --no-same-owner; then
    printf 'Falha ao extrair %s.\n' "$asset" >&2
    exit 1
fi
pkg_dir=$tmp/pkg/dish-$version-linux-x86_64
if [ ! -f "$pkg_dir/install.sh" ]; then
    printf 'Pacote inválido: install.sh não encontrado.\n' >&2
    exit 1
fi

desktop_file=$child_data/applications/dish.desktop
desktop_existed=1
if [ "$action" = install ] && [ ! -e "$desktop_file" ]; then
    desktop_existed=0
fi

if [ "$assume_yes" -eq 0 ] && (exec </dev/tty) 2>/dev/null; then
    if [ "$action" = install ]; then
        printf 'Dish %s será instalado em %s.\n' "$version" "$bin_dir"
    else
        printf 'Dish %s será removido de %s.\n' "$version" "$bin_dir"
    fi
    printf 'Continuar? [S/n] '
    answer=
    IFS= read -r answer < /dev/tty || answer=
    case "$answer" in
        n|N) printf 'Nada foi alterado.\n'; exit 0 ;;
    esac
fi

if [ -n "$prefix" ]; then
    HOME=$prefix XDG_DATA_HOME=$prefix/.local/share bash "$pkg_dir/install.sh" "$action" </dev/null
else
    bash "$pkg_dir/install.sh" "$action" </dev/null
fi

if [ "$action" = install ] && [ "$no_desktop" -eq 1 ] && [ "$desktop_existed" -eq 0 ]; then
    # install-release.sh sempre cria o atalho; removemos apenas o que este
    # instalador acabou de criar, para não apagar um atalho anterior.
    if [ -f "$desktop_file" ]; then
        rm -f "$desktop_file"
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$child_data/applications" >/dev/null 2>&1 || true
    fi
fi

if [ "$action" = install ]; then
    case ":${PATH:-}:" in
        *":$bin_dir:"*) ;;
        *) printf 'Dica: %s não está no PATH; adicione essa pasta ao PATH para executar "dish".\n' "$bin_dir" ;;
    esac
fi
