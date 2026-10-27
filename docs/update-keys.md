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
assinatura. O job de publicação recebe a privada pelo secret, assina e remove o
temporário ao terminar; a verificação com a pública do cliente é feita pelo
comando abaixo ou por uma etapa explícita do job. Só esse job precisa de
`contents: write`; os demais mantêm `contents: read`. Se faltar secret ou a
assinatura não verificar, a publicação deve falhar. Não foi criado um workflow
parcial que pudesse publicar sem essa cadeia completa.

## Gerar, assinar e publicar cada release

Depois de construir e anexar os assets Linux, baixe para um diretório vazio os
arquivos `.tar.gz`, `.deb` e seus dois `.sha256` daquele release. O gerador
não reconstrói binários, não instala nada e não publica nada. Ele sempre assina
em modo de release e recusa gerar o manifesto se a chave não estiver presente:

```sh
# Offline: a privada fica no disco, fora do checkout.
python3 scripts/package-manifest.py /tmp/dish-release-assets \
  --tag v0.1.0-alpha.6 --published-at 2026-10-26T12:00:00Z \
  --sign --secret-key-file "$HOME/.config/dish-release-keys/dish.key"

# CI: MINISIGN_SECRET_KEY já vem do secret do environment release.
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

Após revisão e verificação, o mantenedor publica os dois arquivos juntos:

```sh
gh release upload v0.1.0-alpha.6 \
  /tmp/dish-release-assets/latest.json \
  /tmp/dish-release-assets/latest.json.minisig
```

Nunca use `--clobber`. Para assinatura offline, estes passos são o checklist de
publicação. Para CI, reproduza a mesma sequência no job protegido depois de
validar os assets reais. A chave pública do binário, a pública usada na
verificação da publicação e a privada usada para assinar precisam corresponder.

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
