#!/usr/bin/env bash
# Publica um Release completo do Dish numa ordem segura.
#
# Uso:
#   scripts/publish-release.sh TAG ARTIFACT_DIR [--secret-key-file PATH] [--public-key PATH]
#
# Ordem: confere argumentos, ferramentas, checksums e a chave; anexa os quatro
# assets Linux ao Release; publica o rascunho; baixa de volta os bytes
# publicados; gera e assina latest.json sobre esses bytes; confere nome, tamanho,
# URL e digest contra o Release; sobe latest.json e latest.json.minisig por
# último; baixa e confere tudo de novo.
#
# O manifesto é gerado depois dos assets porque ele referencia a URL e o tamanho
# de cada asset. Enquanto o Release é rascunho, as URLs de download são
# "untagged-..." e não servem para o cliente.
set -euo pipefail

GH=${GH:-gh}
MINISIGN=${MINISIGN:-minisign}
PYTHON=${PYTHON:-python3}
TAG=""
ARTIFACT_DIR=""
SECRET_KEY_FILE=""
PUBLIC_KEY_FILE=""
WORK_DIR=""

usage() {
  printf 'uso: %s TAG ARTIFACT_DIR [--secret-key-file PATH] [--public-key PATH]\n' \
    "$(basename "$0")" >&2
}

die() {
  printf 'erro: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [ -n "${WORK_DIR:-}" ] && [ -d "$WORK_DIR" ]; then
    rm -rf -- "$WORK_DIR"
  fi
}
trap cleanup EXIT

# Repassa --repo ao gh quando GH_REPO estiver definida.
gh_release() {
  if [ -n "${GH_REPO:-}" ]; then
    "$GH" release "$@" --repo "$GH_REPO"
  else
    "$GH" release "$@"
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --secret-key-file)
      [ "$#" -ge 2 ] || die "--secret-key-file exige um caminho"
      SECRET_KEY_FILE=$2
      shift 2
      ;;
    --public-key)
      [ "$#" -ge 2 ] || die "--public-key exige um caminho"
      PUBLIC_KEY_FILE=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --*)
      usage
      die "opção desconhecida: $1"
      ;;
    *)
      if [ -z "$TAG" ]; then
        TAG=$1
      elif [ -z "$ARTIFACT_DIR" ]; then
        ARTIFACT_DIR=$1
      else
        usage
        die "argumentos demais: $1"
      fi
      shift
      ;;
  esac
done

[ -n "$TAG" ] && [ -n "$ARTIFACT_DIR" ] || {
  usage
  die "informe TAG e ARTIFACT_DIR"
}
case "$TAG" in
  v?*) ;;
  *)
    usage
    die "TAG inválida: deve começar com v (ex.: v0.1.0-alpha.6)"
    ;;
esac

version=${TAG#v}
portable="dish-$version-linux-x86_64.tar.gz"
deb="dish-pi_${version}_amd64.deb"
release_assets=("$portable" "$deb" "$portable.sha256" "$deb.sha256")

[ -d "$ARTIFACT_DIR" ] || die "diretório de artifacts não encontrado: $ARTIFACT_DIR"
for name in "${release_assets[@]}"; do
  [ -f "$ARTIFACT_DIR/$name" ] || die "arquivo ausente em $ARTIFACT_DIR: $name"
done

for name in "$portable" "$deb"; do
  if ! (cd "$ARTIFACT_DIR" && sha256sum -c "$name.sha256" >/dev/null); then
    die "checksum inválido para $name em $ARTIFACT_DIR"
  fi
done

for tool in "$GH" "$MINISIGN" "$PYTHON" sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || die "ferramenta não encontrada: $tool"
done

if [ -n "$SECRET_KEY_FILE" ]; then
  [ -r "$SECRET_KEY_FILE" ] || die "chave privada ilegível: $SECRET_KEY_FILE"
elif [ -z "${MINISIGN_SECRET_KEY:-}" ]; then
  die "sem chave privada; nada foi publicado"
fi

# Um único diretório temporário; o trap EXIT apaga a chave, o probe e as cópias.
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/dish-publish.XXXXXX")

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [ -n "$PUBLIC_KEY_FILE" ]; then
  [ -r "$PUBLIC_KEY_FILE" ] || die "chave pública ilegível: $PUBLIC_KEY_FILE"
  public_key=$PUBLIC_KEY_FILE
else
  update_rs="$script_dir/../src/update.rs"
  [ -r "$update_rs" ] || die "não foi possível ler $update_rs; passe --public-key"
  key=$(sed -n 's/^[[:space:]]*pub const UPDATE_PUBLIC_KEY: &str = "\([^"]*\)";.*$/\1/p' "$update_rs")
  key=${key%%$'\n'*}
  [ -n "$key" ] || die "UPDATE_PUBLIC_KEY não encontrada em $update_rs; passe --public-key"
  public_key="$WORK_DIR/dish-update.pub"
  {
    printf 'untrusted comment: dish update key\n'
    printf '%s\n' "$key"
  } > "$public_key"
fi

if [ -n "$SECRET_KEY_FILE" ]; then
  probe_key=$SECRET_KEY_FILE
else
  probe_key="$WORK_DIR/secret.key"
  (umask 077 && printf '%s\n' "$MINISIGN_SECRET_KEY" > "$probe_key")
fi

# Prova a chave antes de qualquer chamada ao GitHub.
probe="$WORK_DIR/probe"
printf 'dish release key check\n' > "$probe"
if ! "$MINISIGN" -S -s "$probe_key" -m "$probe" -x "$probe.minisig" </dev/null >/dev/null 2>&1; then
  die "não foi possível assinar com a chave privada; nada foi publicado"
fi
if ! "$MINISIGN" -V -p "$public_key" -m "$probe" -x "$probe.minisig" >/dev/null 2>&1; then
  die "a chave privada não corresponde à chave pública do cliente; nada foi publicado"
fi

# Ensaio sem assinatura do gerador: um asset recusado (BUILD.json, versão,
# target, glibc ou checksum) não pode publicar o rascunho. Só lê os arquivos.
if ! "$PYTHON" -c '
import importlib.util
import sys
from pathlib import Path
spec = importlib.util.spec_from_file_location("package_manifest", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
try:
    module.manifest(Path(sys.argv[2]), sys.argv[3], "2000-01-01T00:00:00Z")
except (ValueError, OSError) as error:
    print(error, file=sys.stderr)
    sys.exit(1)
' "$script_dir/package-manifest.py" "$ARTIFACT_DIR" "$TAG"; then
  die "o gerador recusa os assets locais; nada foi publicado"
fi

# Depois da publicação, qualquer falha deixa o Release sem manifesto válido.
fail_published() {
  printf 'erro: o Release %s está publicado SEM manifesto válido.\n' "$TAG" >&2
  printf 'O auto-aviso de atualização não vai funcionar até corrigir: remova latest.json e latest.json.minisig do Release à mão e rode este script de novo.\n' >&2
  exit 1
}

release_json="$WORK_DIR/release.json"
gh_release view "$TAG" --json assets,tagName,isDraft,publishedAt > "$release_json" ||
  die "não foi possível ler o Release $TAG (ele existe?)"

if "$PYTHON" -c '
import json
import sys
assets = json.load(open(sys.argv[1], encoding="utf-8")).get("assets") or []
names = {asset.get("name") for asset in assets}
sys.exit(0 if names & {"latest.json", "latest.json.minisig"} else 1)
' "$release_json"; then
  die "manifesto já publicado; nunca substituímos assets"
fi

for name in "${release_assets[@]}"; do
  path="$ARTIFACT_DIR/$name"
  size=$(stat -c %s -- "$path")
  digest=$(sha256sum -- "$path" | cut -d ' ' -f 1)
  state=$("$PYTHON" -c '
import json
import sys
release = json.load(open(sys.argv[1], encoding="utf-8"))
name = sys.argv[2]
size = int(sys.argv[3])
digest = sys.argv[4]
matches = [asset for asset in release.get("assets") or [] if asset.get("name") == name]
if not matches:
    print("missing")
elif len(matches) > 1:
    print("duplicate")
else:
    asset = matches[0]
    if asset.get("size") != size:
        print("size")
    elif asset.get("digest") is not None and asset.get("digest") != "sha256:" + digest:
        print("digest")
    else:
        print("ok")
' "$release_json" "$name" "$size" "$digest") ||
    die "não foi possível conferir o asset $name no Release $TAG"
  case "$state" in
    missing)
      gh_release upload "$TAG" "$path" ||
        die "falha ao enviar $name para o Release $TAG"
      ;;
    ok)
      ;;
    duplicate)
      die "asset duplicado no Release: $name"
      ;;
    size)
      die "tamanho de $name no Release difere do local; nada foi substituído"
      ;;
    digest)
      die "digest de $name no Release difere do local; nada foi substituído"
      ;;
    *)
      die "estado inesperado para o asset $name: $state"
      ;;
  esac
done

is_draft=$("$PYTHON" -c '
import json
import sys
print("true" if json.load(open(sys.argv[1], encoding="utf-8")).get("isDraft") else "false")
' "$release_json")
if [ "$is_draft" = "true" ]; then
  gh_release edit "$TAG" --draft=false ||
    die "falha ao publicar o rascunho $TAG"
fi

gh_release view "$TAG" --json assets,tagName,isDraft,publishedAt > "$release_json" ||
  die "não foi possível reler o Release $TAG depois da publicação"
published_at=$("$PYTHON" -c '
import json
import sys
release = json.load(open(sys.argv[1], encoding="utf-8"))
if release.get("isDraft"):
    sys.exit("isDraft ainda true")
if not release.get("publishedAt"):
    sys.exit("publishedAt vazio")
print(release["publishedAt"])
' "$release_json") || die "o Release $TAG não está publicado; nada mais foi feito"

# Baixa os bytes que o cliente realmente vai usar, não os do diretório local.
download_dir="$WORK_DIR/download"
mkdir -p "$download_dir"
for name in "${release_assets[@]}"; do
  gh_release download "$TAG" --dir "$download_dir" -p "$name" ||
    die "falha ao baixar $name do Release $TAG"
done
for name in "$portable" "$deb"; do
  if ! (cd "$download_dir" && sha256sum -c "$name.sha256" >/dev/null); then
    die "checksum do asset baixado não confere: $name"
  fi
done

manifest_args=("$PYTHON" "$script_dir/package-manifest.py" "$download_dir"
  --tag "$TAG" --published-at "$published_at" --sign)
if [ -n "$SECRET_KEY_FILE" ]; then
  manifest_args+=(--secret-key-file "$SECRET_KEY_FILE")
fi
"${manifest_args[@]}" || fail_published

"$MINISIGN" -V -p "$public_key" -m "$download_dir/latest.json" \
  -x "$download_dir/latest.json.minisig" >/dev/null || fail_published

release_check="$WORK_DIR/release-check.json"
gh_release view "$TAG" --json assets,tagName > "$release_check" || fail_published
"$PYTHON" "$script_dir/package-manifest.py" --check-release \
  "$release_check" "$download_dir/latest.json" || fail_published

# O manifesto só sobe depois de tudo conferido, e por último.
gh_release upload "$TAG" "$download_dir/latest.json" "$download_dir/latest.json.minisig" ||
  fail_published

verify_dir="$WORK_DIR/verify"
mkdir -p "$verify_dir"
for name in latest.json latest.json.minisig; do
  gh_release download "$TAG" --dir "$verify_dir" -p "$name" || fail_published
  cmp "$download_dir/$name" "$verify_dir/$name" || fail_published
done
"$MINISIGN" -V -p "$public_key" -m "$verify_dir/latest.json" \
  -x "$verify_dir/latest.json.minisig" >/dev/null || fail_published

release_final="$WORK_DIR/release-final.json"
gh_release view "$TAG" --json assets,tagName > "$release_final" || fail_published
"$PYTHON" "$script_dir/package-manifest.py" --check-release \
  "$release_final" "$verify_dir/latest.json" || fail_published

printf 'Release %s completo: assets Linux, latest.json e latest.json.minisig conferidos.\n' "$TAG"
