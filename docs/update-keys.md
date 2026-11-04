# Chaves e publicação do manifesto da Fase 1

O contrato é o da Seção 4 de [update-design.md](update-design.md): schema 1,
`version`, `tag`, `channel`, `published_at`, `notes_url`, `target`,
`minimum_glibc` e `assets` com `kind`, `name`, `sha256`, `size`, `url`.
Não há campo de assinatura no JSON. A assinatura destacada cobre os bytes
exatos de `latest.json`, incluindo espaços e a newline final.

A chave pública de produção está embutida no cliente em `src/update.rs`, na
constante `UPDATE_PUBLIC_KEY`, como a linha base64 (sem `untrusted comment`)
gerada por `minisign -G`:

```text
RWTtGSFer94FZx9C2/b1vt6P5eS9t7J3n1xxO9yGSnrS2uTSreg0QgyG
```

O cliente usa `ureq` 3.5 com rustls e `minisign-verify` 0.3. A chave privada
correspondente nunca está no repositório, no archive ou no binário. Não há
variável de ambiente nem opção para trocar a chave ou ignorar a assinatura em
runtime; trocar a chave exige recompilar o cliente. Não publique um cliente
aceitando a chave de teste como chave de produção.

As fixtures em `tests/fixtures/update/` contêm uma chave Ed25519 privada de
teste em PEM (`test-private.pem`, deliberadamente pública), sua pública no
formato minisign e manifestos com assinaturas destacadas minisign modernas
(`ED`, pré-hash BLAKE2b-512). `generate.py` regenera os mesmos bytes usando
Python `cryptography`; esse pacote não é necessário para `cargo test`.
O teste Rust verifica as fixtures pela biblioteca real, com `allow_legacy=false`.
O CLI `minisign` é usado pelos testes de assinatura do gerador quando está
disponível; sem ele, esses testes são pulados. A interoperabilidade com o CLI
também deve ser conferida no checklist de publicação acima de qualquer release
real. A chave de teste nunca é referenciada no caminho de produção.

## Gerar e custodiar a chave real

O dono gera o par fora do checkout, em uma máquina de confiança:

```sh
mkdir -m 700 -p "$HOME/.config/dish-release-keys"
minisign -G \
  -s "$HOME/.config/dish-release-keys/dish.key" \
  -p "$HOME/.config/dish-release-keys/dish.pub"
```

A privada é protegida por senha por padrão. Guarde a privada e sua cópia de
segurança cifrada fora do repositório, do archive e do binário. Nunca registre
a senha ou o conteúdo da privada em logs. A pública pode ser distribuída; a
chave de produção já está embutida em `UPDATE_PUBLIC_KEY`, e mudá-la exige
recompilar o cliente.

Para automação, o secret deve se chamar **`MINISIGN_SECRET_KEY`**, no environment
protegido **`release`** do GitHub Actions, com revisor obrigatório. Não use a
chave privada de teste. Suba o conteúdo da privada sem imprimi-lo:

```sh
gh secret set MINISIGN_SECRET_KEY --repo cleisonsantos/dish --env release \
  < "$HOME/.config/dish-release-keys/dish.key"
```

Se a política do dono permitir assinatura sem interação no CI, gere o par com
`minisign -G -W` em vez do comando acima: `-W` deixa a privada sem senha e sua
proteção passa a depender da custódia do secret. Não remova a senha da chave
offline para automatizar sem essa decisão.

O gerador lê o conteúdo de `MINISIGN_SECRET_KEY` em runtime ou o caminho local
passado com `--secret-key-file`. Ele grava a privada em arquivo temporário com
permissão `0600`, assina os bytes exatos de `latest.json`, grava
`latest.json.minisig` ao lado e apaga o temporário. Sem chave, o comando falha
antes de criar o manifesto: em modo de release nunca existe `latest.json` sem
assinatura.

A publicação hoje é feita pelo script local `scripts/publish-release.sh`
(abaixo), usando a mesma privada. Não existe workflow publicando o manifesto; o
secret no environment `release` continua sendo o caminho para automatizar no CI
no futuro, mas não é usado hoje. O script local verifica a privada contra a
pública de `src/update.rs` antes de qualquer chamada ao GitHub, então uma chave
errada não publica nada.

## Gerar, assinar e publicar cada release

O caminho normal agora é `scripts/publish-release.sh TAG ARTIFACT_DIR`; o
`ARTIFACT_DIR` é o artifact Linux do CI já extraído. A ordem é obrigatória:
subir os binários → publicar → gerar o manifesto a partir dos assets baixados →
anexar `latest.json` + `.minisig` por último → conferir. O script executa essa
sequência e falha fechado em cada etapa.

Com a privada no disco, fora do checkout:

```sh
scripts/publish-release.sh v0.1.0-alpha.6 /tmp/dish-release-assets \
  --secret-key-file "$HOME/.config/dish-release-keys/dish.key"
```

Com `MINISIGN_SECRET_KEY` já no ambiente:

```sh
export MINISIGN_SECRET_KEY="$(cat "$HOME/.config/dish-release-keys/dish.key")"
scripts/publish-release.sh v0.1.0-alpha.6 /tmp/dish-release-assets
```

Não imprima o conteúdo da privada nem o inclua em logs. Sem chave, o script
para antes de qualquer chamada ao GitHub. Ele também confere os dois `.sha256`
locais, exige que um asset já presente tenha o mesmo tamanho (e o mesmo digest,
quando o GitHub informa um), nunca usa `--clobber`, publica o rascunho e relê o
`publishedAt` real. Depois baixa os quatro assets Linux do próprio Release, gera
e assina o manifesto sobre esses bytes, confere nome/tamanho/URL/digest com
`--check-release` e só então envia os dois arquivos do manifesto. No fim baixa
tudo de novo, compara, verifica a assinatura e repete a conferência.

Os comandos manuais abaixo continuam servindo de referência e de conferência.
Para o caminho manual, depois de os assets Linux estarem anexados ao Release
publicado, baixe para um diretório vazio os arquivos `.tar.gz`, `.deb` e seus
dois `.sha256`. O gerador não reconstrói binários, não instala nada e não publica
nada. Ele sempre assina em modo de release e recusa gerar o manifesto se a chave
não estiver presente:

```sh
# Offline: a privada fica no disco, fora do checkout.
python3 scripts/package-manifest.py /tmp/dish-release-assets \
  --tag v0.1.0-alpha.6 --published-at 2026-10-26T12:00:00Z \
  --sign --secret-key-file "$HOME/.config/dish-release-keys/dish.key"

# CI futuro: MINISIGN_SECRET_KEY viria do secret do environment release.
python3 scripts/package-manifest.py /tmp/dish-release-assets \
  --tag v0.1.0-alpha.6 --published-at 2026-10-26T12:00:00Z --sign

minisign -V -p "$HOME/.config/dish-release-keys/dish.pub" \
  -m /tmp/dish-release-assets/latest.json \
  -x /tmp/dish-release-assets/latest.json.minisig
```

Use a tag e o horário UTC reais da publicação; os valores acima são exemplos.
O gerador exige os dois formatos Linux e valida checksum, tamanho positivo,
nome, versão, target e glibc do `BUILD.json`. O JSON é determinístico: chaves
ordenadas, indentação de dois espaços e newline final. Não altere esses bytes
após assinar. A assinatura e o manifesto não entram em `assets`.

Após revisão e verificação, o mantenedor publica os dois arquivos juntos, por
último:

```sh
gh release upload v0.1.0-alpha.6 \
  /tmp/dish-release-assets/latest.json \
  /tmp/dish-release-assets/latest.json.minisig
```

Nunca use `--clobber`. A conferência final compara o manifesto com a lista real
de assets do Release:

```sh
gh release view v0.1.0-alpha.6 --json assets,tagName > /tmp/dish-release.json
python3 scripts/package-manifest.py --check-release \
  /tmp/dish-release.json /tmp/dish-release-assets/latest.json
```

`--check-release` falha se a tag, o nome, o tamanho, a URL ou o digest de um
asset Linux não baterem com o manifesto. A chave pública do binário, a pública
usada na verificação e a privada usada para assinar precisam corresponder. O
`latest.json` só pode ser gerado depois da publicação: enquanto o Release for
rascunho, as URLs são `untagged-...` e não servem para o cliente.

## Versão do binário

`src/version.rs::release_version()` lê `DISH_RELEASE_VERSION` com `option_env!`
e cai em `CARGO_PKG_VERSION` quando ausente. Sobre mostra `(build local)` nesse
caso. `Cargo.toml` continua em `0.1.0`; não o sincronize com cada alpha.
O empacotador Linux injeta seu rótulo no build. O arquivo auxiliar
`target/x86_64-unknown-linux-gnu/release/dish.release.json` liga o rótulo ao hash
do binário: `--skip-build` exige essa identidade e recusa outro rótulo ou um
binário alterado. Esse auxiliar não é instalado.

A opção `dish --version` e mudanças no parser de argumentos estão fora da
issue #118 conforme o briefing, mesmo que o design original as descreva.
