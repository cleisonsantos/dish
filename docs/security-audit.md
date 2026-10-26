# Auditoria de segurança — issue #90

Data: 2026-10-25 (America/Sao_Paulo). Worktree: `dish-issue90`, branch
`fix/90-dependency-audit`, base auditada `e6bc9f8bd60982e6ba701b02ba3130b65b706caa`.
SHA-256 do `Cargo.lock`: `449752b0b7767371ca809ece422450d127c97b004631aa2ee41a5bf8c0ad86c4`.
Nenhum commit, push ou alteração da issue foi realizado.

**Resultado RustSec: 0 vulnerabilidades; 1 aviso unsound e 5 unmaintained.**
Críticas = 0, altas = 0, médias = 0, baixas = 0, sem severidade = 0,
desconhecidas = 0 (contagem de vulnerabilidades; os seis avisos são separados).
A auditoria global permanece incompleta pela cobertura de licenças multiplataforma.

## Dependências: rodada 2, dados reais

Dados fornecidos pelo coordenador em `audit-90.json`, reconfirmados localmente em
2026-10-25 com cargo-audit 0.22.2, sem downloads. Advisory DB: 1352 avisos,
commit `21d909e4c8bd4e14c7aea58bf980c881a0053ab1`, atualizado em
2026-10-24T10:30:13+02:00. O commit foi também conferido com
`git -C ~/.cargo/advisory-db rev-parse HEAD`. O JSON da execução `--no-fetch`
traz `last-commit`/`last-updated` nulos; os metadados acima vêm do JSON do
coordenador e do Git local, não de campos preenchidos artificialmente.
As seções `vulnerabilities` e `warnings` das duas execuções são idênticas.

```sh
/tmp/dish-security-tools/bin/cargo-audit audit --file Cargo.lock --json --no-fetch --no-yanked --db ~/.cargo/advisory-db
cargo metadata --locked --offline --filter-platform x86_64-unknown-linux-gnu --format-version 1
rg -n 'AsyncFd|tokio' src
```

A busca em `src/` não teve correspondências (exit 1). As origens abaixo foram
obtidas das arestas reversas de `resolve.nodes` do metadata offline. Para Tokio,
ausente desse inventário, foram usadas as arestas de `Cargo.lock`; o pacote
`wasm-bindgen-futures` não tem fontes locais disponíveis. Uma dependência no
lockfile não demonstra que a função afetada seja chamada.

| Aviso / crate | Origem confirmada (Dish → dependência) | Alcançabilidade e decisão |
| --- | --- | --- |
| RUSTSEC-2026-0386 · tokio 1.53.1 · unsound | `gpui_ce_platform → gpui_ce_web/gpui_ce_wgpu → wasm-bindgen-futures 0.4.79 → tokio`; também via `wgpu 29.0.4 → wasm-bindgen-futures` no lock | Nenhum `AsyncFd` ou `tokio` em `src/`; Tokio ausente do metadata Linux. Aceitar no escopo Linux atual: os construtores Unix afetados não estão nesse grafo. Não é prova para todos os targets/features. Recomendar atualização futura para 1.53.3 ou superior compatível, sem executar alteração do lock nesta rodada. |
| RUSTSEC-2024-0436 · paste 1.0.15 · unmaintained | `gpui-ce/gpui_ce_linux → image 0.25.10 → exr 1.74.2 → pulp 0.22.3 → paste`; também `image → ravif 0.13.0 → rav1e 0.8.1 → paste` | Proc macro usado na compilação das dependências de imagens; não há função vulnerável identificada pelo aviso. Aceitar temporariamente o risco de manutenção; acompanhar migração upstream para alternativa mantida (ex.: pastey). Não vem da cadeia de fontes. |
| RUSTSEC-2026-0192 · ttf-parser 0.25.1 · unmaintained | `gpui_ce_platform → gpui_ce_linux → gpui_ce_wgpu → cosmic-text 0.19.0 → fontdb 0.23.0 → ttf-parser` | Parsing de fontes potencialmente alcançável na UI; não alegar código morto. Aviso de manutenção, sem função vulnerável apontada. Aceitar temporariamente e acompanhar migração upstream para skrifa. |
| RUSTSEC-2026-0371 · zbus_names 4.3.4 · unmaintained | `gpui_ce_platform → gpui_ce_linux → accesskit_unix/ashpd/notify-rust/oo7 → zbus 5.19.0 → zbus_names`; também AT-SPI, zbus_macros e zbus_xml | Tipos de nomes D-Bus potencialmente alcançáveis em acessibilidade, portais, notificações e integração de credenciais; sem falha concreta indicada. Aceitar temporariamente, acompanhar migração coordenada upstream para `zbus 6::names`. |
| RUSTSEC-2026-0370 · zvariant 5.15.0 · unmaintained | Mesma integração Linux, via `zbus`, `zbus_names`, AT-SPI, `accesskit_atspi_common`, `oo7`, macros e ferramentas zbus | Serialização D-Bus/GVariant potencialmente alcançável. Aceitar temporariamente risco de manutenção; recomendar migração upstream para `zbus 6::wire` e avaliar `zgvariant` para GVariant (feature ativa). |
| RUSTSEC-2026-0372 · zvariant_derive 5.15.0 · unmaintained | Cadeia anterior → `zvariant 5.15.0 → zvariant_derive` | Proc macro na compilação; código gerado pode ser executado na integração D-Bus. Aceitar temporariamente, migrar junto com zvariant para os derives de zbus 6. |

Os cinco avisos de manutenção não indicam versões corrigidas (`patched: []`).
Não há upgrade comprovado que preserve o lock atual: zbus 6 exige coordenação
com dependentes; a resolução e os testes de qualquer migração ficam para outra
rodada. Para Tokio, o advisory lista `>=1.51.6, <1.52.0` e `>=1.53.3` como
corrigidos. Não se realizou prova dinâmica de alcançabilidade nem análise de
todos os targets. Nenhum `--ignore` foi adicionado; os seis avisos continuam
visíveis e produzem bit de achados no script.

## Licenças: cobertura real, não uma lista presumida

Esteira inspecionada: `scripts/license_bundle.py`, `scripts/package-linux.py`,
`scripts/package-windows.py`, `scripts/package-deb.py`, `licenses/overrides.json`,
`licenses/native.json`, `THIRD_PARTY_NOTICES.md`, `docs/releasing.md` e
`tests/test_release_package.py`.

```sh
cargo metadata --locked --offline --format-version 1
cargo metadata --locked --offline --filter-platform x86_64-unknown-linux-gnu --format-version 1
scripts/security-audit.sh
```

O primeiro metadata falhou:

```text
error: failed to download `accesskit_macos v0.26.3`

Caused by:
  attempting to make an HTTP request, but --offline was specified
```

O segundo funcionou: 511 pacotes incluindo Dish, ou **510 dependências Linux**.
O lockfile contém 698 pacotes incluindo Dish, ou **697 dependências**.
O script compara nome, versão e origem; não confunde o filtro Linux com cobertura
completa do lockfile. Os **187 pacotes fora desse inventário** estão identificados
como `LICENSE_UNVERIFIED` na saída integral abaixo. Isso não prova que lhes falta
uma licença: indica que suas licenças não foram verificadas nesta execução.

A rotina real `collect_licenses` foi executada para cada uma das 510 dependências,
em diretório temporário, sem alterar a rotina de release. As 510 entradas foram
coletadas; **zero falhas de texto/hash/fonte e zero expressões fora da política
revisada nesse subconjunto**. Não foi necessário alterar overrides nem listas de
licenças. Não se afirma cobertura dos demais 187 pacotes.

A política do helper aceita as opções permissivas explícitas, respeita `AND` e
`OR`, permite MPL-2.0 para `option-ext` com a fonte exigida pelo coletor, e escolhe
a opção Apache-2.0 de `self_cell`. Os hashes e avisos nativos de FreeType são
checados pelo coletor existente. Uma expressão desconhecida exige revisão e
retorno não zero. Isso é uma checagem da política documentada do projeto, não
parecer jurídico. A coleta de licenças do toolchain não foi executada: ela baixa
textos no commit exato do Rust e exige a rede indisponível aqui.

## Segredos: resultado e limites

Gitleaks indisponível; a tentativa de acessar sua release falhou. Foi executado
scanner determinístico Python, com regex para IDs AWS AKIA/ASIA e atribuições de AWS secret access keys, GitHub PATs,
OpenAI/Anthropic, cabeçalhos de chaves privadas e senhas em URLs. O código está em
`scripts/security_audit.py`, sem dependências Python externas.

Cobertura: `git ls-files -z` no worktree e `git log --all --full-history --root -m
-p --no-ext-diff --no-textconv --no-renames`, incluindo commits iniciais, removidos
e diffs de merges em todas as refs locais. Foram **148 arquivos versionados,
3 binários ignorados, 195 commits, clone não raso**. Exclui `target/`; não executa
textconv nem diff externo. Conteúdos correspondentes nunca são impressos.

Resultado bruto: **22 ocorrências**, sendo 4 no worktree e 18 nos diffs históricos,
correspondentes a **3 valores distintos**. Revisão dos contextos confirmou
marcadores sintéticos de testes em `src/rpc.rs` (módulo de teste),
`tests/diagnostic_privacy_smoke.py`, `tests/fixtures/fake_pi.py` e
`tests/fixtures/fake-pi-sdk/dist/index.js`. Não foram removidos nem ignorados pelo
scanner. Decisão: preservar essas fixtures; não são credenciais reais. Não há
segredo real confirmado entre os candidatos. O scanner mantém retorno 1 para
candidatos, mesmo os já revisados, evitando uma exclusão ampla de testes.

Não cobre arquivos não versionados, mensagens de commit, objetos Git inalcançáveis/reflogs, refs remotas
não baixadas, conteúdo de binários, submódulos/LFS externos, tokens codificados ou
formatos fora das regex. Não valida credenciais contra provedores. A contagem de
195 commits é das refs locais no momento da execução e pode crescer. Os novos
arquivos desta entrega ainda não são versionados e foram verificados separadamente.

## Artefatos reais inspecionados, sem instalação

Arquivos locais fornecidos pelo coordenador em
`/home/cachy/.hermes/profiles/sureka-cloud/cache/scratch/dish-issues/artifact90/`.
SHA-256 recalculado de cada arquivo e comparado com seu `.sha256`:

| Arquivo | Bytes | SHA-256 | Resultado |
| --- | ---: | --- | --- |
| dish-0.1.0-alpha.5-linux-x86_64.tar.gz | 17140383 | `89d5f2834ed7ca193cd738481c1c9061027690125e4bd2dba814c89dd4742524` | confere |
| dish-pi_0.1.0-alpha.5_amd64.deb | 13433234 | `827b846f50b7507fdcd5c6019395744e7e86cda4cd4410b956d9d0d65b4f8185` | confere |

Inspeção somente leitura com `tarfile` e leitura dos membros ar do Debian,
sem extrair/executar instaladores. Listas completas locais, com modos e tamanhos:
`/tmp/dish90-tar-files.txt`, `/tmp/dish90-deb-data-files.txt` e
`/tmp/dish90-deb-control-files.txt`; procedimento em `/tmp/dish90-inspect.py`.

- Tar: 1482 membros, 941 arquivos. Na raiz `dish-0.1.0-alpha.5-linux-x86_64/`:
  `BUILD.json`, `README.md`, `bin/dish`, `install.sh`, `uninstall.sh`,
  `share/icons/dish.png` e `share/licenses/`.
- Debian: ar contém somente `debian-binary`, `control.tar.zst`, `data.tar.zst`.
  Control contém apenas diretório e `control`, sem scripts de manutenção.
  Data contém 1489 membros, 941 arquivos: `usr/bin/dish-pi`, desktop e ícone
  em `usr/share/applications/` e `usr/share/icons/hicolor/512x512/apps/`,
  documentação (`LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.md`, `changelog`,
  `copyright`) e `licenses/` em `usr/share/doc/dish-pi/`.
- Zero membros setuid/setgid, links, devices, caminhos absolutos ou `..`;
  zero nomes duplicados. Nenhum arquivo inesperado na estrutura inspecionada,
  comparada ao layout de `package-linux.py`/`package-deb.py`.
- `BUILD.json` do tar: versão `0.1.0-alpha.5`, target
  `x86_64-unknown-linux-gnu`, glibc mínima 2.39, 510 dependências,
  rustc `1.99.1 (58cf8f9c3 2026-10-13)`. O `cargo_lock_sha256` coincide com
  o lock desta branch. Não há `Cargo.lock` embarcado: conferiu-se a declaração
  do hash, não uma reprodução do build ou hash de um lock extraído.
- **O Debian não embarca BUILD.json**, conforme o empacotador atual. Seu
  `control` declara `0.1.0~alpha.5`, `amd64`, glibc >= 2.39, compatíveis com
  o tar. O binário é idêntico byte a byte ao tar; não se inferiu um manifesto
  inexistente. Incluir BUILD.json no Debian é recomendação para outra rodada.
- Licenças presentes: LICENSE do projeto, THIRD_PARTY_NOTICES,
  `dependencies.json` com 510 entradas, todos os textos e fontes referenciados,
  avisos nativos e `rust-toolchain/` (incluindo COPYRIGHT e licenças).
  Conferidos os quatro hashes de `rust-toolchain/provenance.json`.
  Todos os 935 arquivos de licenças mais o binário coincidem byte a byte entre
  os pacotes (936 comparações). Isso não audita as 187 crates de outros targets.

Não houve download, assinatura, instalação nem execução dos binários. A
comparação com checksums fornecidos confirma integridade em relação aos sidecars;
não é validação independente de assinatura/proveniência da publicação.

## Reproduzir

Requisitos: Bash, Python 3.11+, Git, Cargo, fontes locais e cargo-audit 0.22.2
com advisory DB provisionado. Detecção: `CARGO_AUDIT`, ou `cargo-audit` no PATH,
ou `/tmp/dish-security-tools/bin/cargo-audit`. `CARGO_AUDIT_DB` substitui
`~/.cargo/advisory-db`. O executável detectado é chamado diretamente com
`audit --no-fetch --no-yanked --db ...`; não instala nem baixa nada.
`--no-yanked` desativa a consulta separada ao índice de crates revogadas:
**crates yanked não foram verificadas**. O resultado vale para o snapshot local,
não para advisories publicados depois dele.

```sh
bash scripts/security-audit.sh
bash scripts/security-audit.sh --only dependencies
bash scripts/security-audit.sh --only licenses
bash scripts/security-audit.sh --only secrets
```

Códigos: 0 = auditado e limpo; 1 = achados; 2 = incompleto; 3 = ambos.
Avisos e candidatos revisados continuam produzindo achados; ausência/falha da
ferramenta não vira sucesso. Falha operacional com achados preserva os dois bits.
Nenhum job de CI foi alterado. O helper não substitui inspeção de artefatos,
triagem humana ou Gitleaks.

## Saída integral do script — rodada 2

Comando: `bash scripts/security-audit.sh`. Exit **3**.

```text
COMMIT e6bc9f8bd60982e6ba701b02ba3130b65b706caa
LOCK_SHA256 449752b0b7767371ca809ece422450d127c97b004631aa2ee41a5bf8c0ad86c4
WARNING unmaintained {'name': 'paste', 'version': '1.0.15', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': '57c0d7b74b563b49d38dae00a0c37d4d6de9b432382b2892f0574ddcae73fd0a', 'replace': None} RUSTSEC-2024-0436
WARNING unmaintained {'name': 'ttf-parser', 'version': '0.25.1', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': 'd2df906b07856748fa3f6e0ad0cbaa047052d4a7dd609e231c4f72cee8c36f31', 'dependencies': [{'name': 'core_maths', 'version': '0.1.1', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}], 'replace': None} RUSTSEC-2026-0192
WARNING unmaintained {'name': 'zbus_names', 'version': '4.3.4', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': 'd8bf88b4a3ff53e883001e0e0115b297a9d53c31b9c1edd2bfdd853e3428624e', 'dependencies': [{'name': 'serde', 'version': '1.0.229', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'winnow', 'version': '1.0.4', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'zvariant', 'version': '5.15.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}], 'replace': None} RUSTSEC-2026-0371
WARNING unmaintained {'name': 'zvariant', 'version': '5.15.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': 'c1d34c27cc6cdd1f458427519dd6b8612f7b7e3f7b9a0b2355d041dda9869147', 'dependencies': [{'name': 'endi', 'version': '1.1.1', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'enumflags2', 'version': '0.7.12', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'serde', 'version': '1.0.229', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'serde_bytes', 'version': '0.11.19', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'winnow', 'version': '1.0.4', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'zcheapstr', 'version': '1.1.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'zvariant_derive', 'version': '5.15.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'zvariant_utils', 'version': '4.2.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}], 'replace': None} RUSTSEC-2026-0370
WARNING unmaintained {'name': 'zvariant_derive', 'version': '5.15.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': '864155e69b4352db0c7f374917bf45d1e0c8d17659c8b3dbf9795f3673f8c497', 'dependencies': [{'name': 'proc-macro-crate', 'version': '3.5.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'proc-macro2', 'version': '1.0.107', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'quote', 'version': '1.0.47', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'syn', 'version': '3.0.6', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}, {'name': 'zvariant_utils', 'version': '4.2.0', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}], 'replace': None} RUSTSEC-2026-0372
WARNING unsound {'name': 'tokio', 'version': '1.53.1', 'source': 'registry+https://github.com/rust-lang/crates.io-index', 'checksum': '202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed', 'dependencies': [{'name': 'pin-project-lite', 'version': '0.2.17', 'source': 'registry+https://github.com/rust-lang/crates.io-index'}], 'replace': None} RUSTSEC-2026-0386
DEPENDENCIES critical=0 high=0 medium=0 low=0 none=0 unknown=0
CARGO_AUDIT_JSON {"database": {"advisory-count": 1352, "last-commit": null, "last-updated": null}, "lockfile": {"dependency-count": 698}, "settings": {"ignore": [], "informational_warnings": ["unmaintained", "unsound", "notice"], "severity": null, "target_arch": [], "target_os": []}, "vulnerabilities": {"count": 0, "found": false, "list": []}, "warnings": {"unmaintained": [{"advisory": {"aliases": [], "categories": [], "collection": "crates", "cvss": null, "date": "2024-10-07", "description": "The creator of the crate `paste` has stated in the [`README.md`](https://github.com/dtolnay/paste/blob/master/README.md) \nthat this project is not longer maintained as well as archived the repository\n\n## Possible Alternative(s)\n\n- [`pastey`]: a fork of paste and is aimed to be a drop-in replacement with additional features for paste crate\n- [`with_builtin_macros`]: crate providing a [superset of `paste`'s functionality including general `macro_rules!` eager expansions](https://docs.rs/with_builtin_macros/0.1.0/with_builtin_macros/macro.with_eager_expansions.html)  and `concat!`/`concat_idents!` macros\n\n[`pastey`]: https://crates.io/crates/pastey\n[`with_builtin_macros`]: https://crates.io/crates/with_builtin_macros", "expect-deleted": false, "id": "RUSTSEC-2024-0436", "informational": "unmaintained", "keywords": [], "license": "CC0-1.0", "package": "paste", "references": [], "related": [], "source": null, "title": "paste - no longer maintained", "url": "https://github.com/dtolnay/paste", "withdrawn": null}, "affected": null, "kind": "unmaintained", "package": {"checksum": "57c0d7b74b563b49d38dae00a0c37d4d6de9b432382b2892f0574ddcae73fd0a", "name": "paste", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.15"}, "versions": {"patched": [], "unaffected": []}}, {"advisory": {"aliases": [], "categories": [], "collection": "crates", "cvss": null, "date": "2026-06-28", "description": "The author of `ttf-parser` has stated that the crate is unmaintained and will not receive further fixes (see the referenced issue).\n\n## Alternative(s)\n\n- [`skrifa`](https://crates.io/crates/skrifa), an actively maintained TrueType and OpenType font parsing crate, part of the Google Fonts \"oxidize\" (`fontations`) project.", "expect-deleted": false, "id": "RUSTSEC-2026-0192", "informational": "unmaintained", "keywords": [], "license": "CC0-1.0", "package": "ttf-parser", "references": [], "related": [], "source": null, "title": "`ttf-parser` is unmaintained", "url": "https://github.com/harfbuzz/ttf-parser/issues/217", "withdrawn": null}, "affected": null, "kind": "unmaintained", "package": {"checksum": "d2df906b07856748fa3f6e0ad0cbaa047052d4a7dd609e231c4f72cee8c36f31", "dependencies": [{"name": "core_maths", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "0.1.1"}], "name": "ttf-parser", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "0.25.1"}, "versions": {"patched": [], "unaffected": []}}, {"advisory": {"aliases": [], "categories": [], "collection": "crates", "cvss": null, "date": "2026-10-16", "description": "The `zbus_names` crate is no longer maintained as a standalone crate.\n\nIt has been merged into [`zbus`](https://crates.io/crates/zbus) starting with version `6.0.0`, as its `names` module.\n\nUsers should migrate to [`zbus`](https://crates.io/crates/zbus), with its default features disabled if they only need the D-Bus name types.", "expect-deleted": false, "id": "RUSTSEC-2026-0371", "informational": "unmaintained", "keywords": [], "license": "CC0-1.0", "package": "zbus_names", "references": [], "related": [], "source": null, "title": "`zbus_names` is unmaintained", "url": "https://github.com/z-galaxy/zbus/issues/2043", "withdrawn": null}, "affected": null, "kind": "unmaintained", "package": {"checksum": "d8bf88b4a3ff53e883001e0e0115b297a9d53c31b9c1edd2bfdd853e3428624e", "dependencies": [{"name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.229"}, {"name": "winnow", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.4"}, {"name": "zvariant", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "5.15.0"}], "name": "zbus_names", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "4.3.4"}, "versions": {"patched": [], "unaffected": []}}, {"advisory": {"aliases": [], "categories": [], "collection": "crates", "cvss": null, "date": "2026-10-16", "description": "The `zvariant` crate is no longer maintained as a standalone crate.\n\nIt has been merged into [`zbus`](https://crates.io/crates/zbus) starting with version `6.0.0`, as its `wire` module.\n\nUsers should migrate to [`zbus`](https://crates.io/crates/zbus), with its default features disabled if they only need the D-Bus wire format. Users of the GVariant format should migrate to [`zgvariant`](https://crates.io/crates/zgvariant).", "expect-deleted": false, "id": "RUSTSEC-2026-0370", "informational": "unmaintained", "keywords": [], "license": "CC0-1.0", "package": "zvariant", "references": [], "related": [], "source": null, "title": "`zvariant` is unmaintained", "url": "https://github.com/z-galaxy/zbus/issues/2043", "withdrawn": null}, "affected": null, "kind": "unmaintained", "package": {"checksum": "c1d34c27cc6cdd1f458427519dd6b8612f7b7e3f7b9a0b2355d041dda9869147", "dependencies": [{"name": "endi", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.1.1"}, {"name": "enumflags2", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "0.7.12"}, {"name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.229"}, {"name": "serde_bytes", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "0.11.19"}, {"name": "winnow", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.4"}, {"name": "zcheapstr", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.1.0"}, {"name": "zvariant_derive", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "5.15.0"}, {"name": "zvariant_utils", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "4.2.0"}], "name": "zvariant", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "5.15.0"}, "versions": {"patched": [], "unaffected": []}}, {"advisory": {"aliases": [], "categories": [], "collection": "crates", "cvss": null, "date": "2026-10-16", "description": "The `zvariant_derive` crate is no longer maintained as a standalone crate.\n\nIts derive macros are provided by [`zbus`](https://crates.io/crates/zbus) starting with version `6.0.0`.\n\nUsers should migrate to [`zbus`](https://crates.io/crates/zbus), with its default features disabled if they only need the D-Bus wire format.", "expect-deleted": false, "id": "RUSTSEC-2026-0372", "informational": "unmaintained", "keywords": [], "license": "CC0-1.0", "package": "zvariant_derive", "references": [], "related": [], "source": null, "title": "`zvariant_derive` is unmaintained", "url": "https://github.com/z-galaxy/zbus/issues/2043", "withdrawn": null}, "affected": null, "kind": "unmaintained", "package": {"checksum": "864155e69b4352db0c7f374917bf45d1e0c8d17659c8b3dbf9795f3673f8c497", "dependencies": [{"name": "proc-macro-crate", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "3.5.0"}, {"name": "proc-macro2", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.107"}, {"name": "quote", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.0.47"}, {"name": "syn", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "3.0.6"}, {"name": "zvariant_utils", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "4.2.0"}], "name": "zvariant_derive", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "5.15.0"}, "versions": {"patched": [], "unaffected": []}}], "unsound": [{"advisory": {"aliases": [], "categories": ["memory-corruption"], "collection": "crates", "cvss": null, "date": "2026-10-13", "description": "`tokio::io::unix::AsyncFd` provides safe constructors (`new`, `with_interest`, `try_new`, and `try_with_interest`) that accept any `T: AsRawFd` rather than requiring proof of ownership or validity of the file descriptor. Because `RawFd` implements `AsRawFd`, safe code can construct an `AsyncFd` from an arbitrary or closed file descriptor, or close/replace the underlying file descriptor while the `AsyncFd` is still alive.\n\nThis violates I/O safety in two ways:\n\n1. `AsyncFd<T>` implements `AsFd` for all `T: AsRawFd` by calling `BorrowedFd::borrow_raw(self.as_raw_fd())`, allowing safe code to obtain an arbitrary `BorrowedFd<'_>` without owning the file descriptor.\n2. Tokio's I/O driver stores a pointer to an internal allocation in the `epoll`/`kqueue` user data for registered file descriptors. If the underlying file descriptor is duplicated and then closed while an `AsyncFd` is alive, and the OS reuses that file descriptor number for a another IO resource on the same runtime, dropping the first `AsyncFd` deregisters the reused file descriptor number and frees the first while the original file description remains registered in `epoll`, leading to a use-after-free when a readiness event fires.\n\nStarting in Tokio 1.51.6, 1.53.3, and 1.54.0, `AsyncFd::new`, `AsyncFd::with_interest`, `AsyncFd::try_new`, and `AsyncFd::try_with_interest` are deprecated in favor of the `unsafe` constructors `AsyncFd::register` and `AsyncFd::register_with_interest`. Callers must ensure that the file descriptor remains valid, open, and unchanged for the lifetime of the `AsyncFd`.", "expect-deleted": false, "id": "RUSTSEC-2026-0386", "informational": "unsound", "keywords": ["io-safety", "use-after-free"], "license": "CC0-1.0", "package": "tokio", "references": ["https://github.com/tokio-rs/tokio/issues/8538", "https://github.com/tokio-rs/tokio/issues/8539"], "related": [], "source": null, "title": "`AsyncFd::new` and related constructors violate I/O safety", "url": "https://github.com/tokio-rs/tokio/pull/8587", "withdrawn": null}, "affected": {"arch": [], "functions": {"tokio::io::unix::AsyncFd::new": [">=0.3.2"], "tokio::io::unix::AsyncFd::try_new": [">=1.37.0"], "tokio::io::unix::AsyncFd::try_with_interest": [">=1.37.0"], "tokio::io::unix::AsyncFd::with_interest": [">=0.3.2"]}, "os": []}, "kind": "unsound", "package": {"checksum": "202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed", "dependencies": [{"name": "pin-project-lite", "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "0.2.17"}], "name": "tokio", "replace": null, "source": "registry+https://github.com/rust-lang/crates.io-index", "version": "1.53.1"}, "versions": {"patched": [">=1.51.6, <1.52.0", ">=1.53.3"], "unaffected": ["<0.3.2"]}}]}}
LICENSES: full offline metadata unavailable; trying Linux inventory
LICENSE_UNVERIFIED accesskit_consumer 0.37.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED accesskit_macos 0.26.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED accesskit_windows 0.33.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED android_system_properties 0.1.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED arbitrary 1.4.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED bindgen 0.71.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED bitflags 1.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED block 0.1.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED block2 0.5.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED block2 0.6.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cbindgen 0.28.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cexpr 0.6.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cgl 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED clang-sys 1.9.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cocoa 0.25.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cocoa 0.26.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cocoa-foundation 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED cocoa-foundation 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED console_error_panic_hook 0.1.7 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-foundation 0.10.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-foundation 0.9.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-foundation-sys 0.8.7 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics 0.23.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics 0.24.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics-helmer-fork 0.24.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics-types 0.1.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics-types 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-graphics2 0.5.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-text 21.0.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED core-video 0.5.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED deranged 0.5.8 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED dispatch 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED dispatch2 0.3.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED dunce 1.0.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED dwrote 0.11.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED foreign-types 0.5.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED foreign-types-macros 0.2.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED foreign-types-shared 0.3.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED gl_generator 0.14.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED glob 0.3.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED glutin_wgl_sys 0.6.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED gpui_ce_macos 0.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED gpui_ce_media 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED gpui_ce_web 0.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED gpui_ce_windows 0.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED heck 0.4.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED hermit-abi 0.5.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED home 0.5.12 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED iana-time-zone-haiku 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED interpolate_name 0.2.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED io-surface 0.16.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED itertools 0.13.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED jni-sys 0.3.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED jni-sys 0.4.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED jni-sys-macros 0.4.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED js-sys 0.3.106 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED khronos_api 3.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED leak 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED leaky-cow 0.1.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED libfuzzer-sys 0.4.13 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED libredox 0.1.25 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED linux-raw-sys 0.4.15 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED mac-notification-sys 0.6.15 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED mach2 0.5.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED malloc_buf 0.0.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED memoffset 0.9.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED metal 0.33.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED minimal-lexical 0.2.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED ndk-sys 0.6.0+11769913 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED nom 7.1.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED ntapi 0.4.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED num-conv 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc 0.2.7 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc-foundation 0.1.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc-sys 0.3.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2 0.5.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2 0.6.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-app-kit 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-app-kit 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-core-data 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-core-foundation 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-core-image 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-core-location 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-encode 4.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-foundation 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-foundation 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-metal 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-metal 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-quartz-core 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-quartz-core 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc2-user-notifications 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc_exception 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED objc_id 0.1.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED portable-atomic 1.15.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED portable-atomic-util 0.2.8 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED powerfmt 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED prettyplease 0.2.37 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED r-efi 5.3.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED r-efi 6.0.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED range-alloc 0.1.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED raw-window-metal 1.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED redox_syscall 0.5.18 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED redox_users 0.4.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED rustix 0.38.44 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED screencapturekit 0.2.8 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED screencapturekit-sys 0.2.8 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED serde_spanned 0.6.9 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED shlex 1.3.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED tao-core-video-sys 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED tauri-winrt-notification 0.8.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED tempfile 3.27.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED time 0.3.55 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED time-core 0.1.9 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED tokio 1.53.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED toml 0.8.23 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED toml_datetime 0.6.11 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED toml_edit 0.22.27 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED toml_write 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED uds_windows 1.2.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED vswhom 0.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED vswhom-sys 0.1.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wasi 0.11.1+wasi-snapshot-preview1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wasip2 1.0.4+wasi-0.2.12 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wasm-bindgen-futures 0.4.79 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED web-sys 0.3.106 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wgpu-core-deps-apple 29.0.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wgpu-core-deps-emscripten 29.0.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED which 6.0.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winapi 0.3.9 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winapi-i686-pc-windows-gnu 0.4.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winapi-util 0.1.11 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winapi-x86_64-pc-windows-gnu 0.4.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows 0.57.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows 0.61.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows 0.62.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-capture 1.5.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-collections 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-collections 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-core 0.57.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-core 0.61.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-core 0.62.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-future 0.2.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-future 0.3.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-implement 0.57.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-implement 0.60.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-interface 0.57.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-interface 0.59.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-link 0.1.3 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-link 0.2.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-numerics 0.2.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-numerics 0.3.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-registry 0.6.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-result 0.1.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-result 0.3.4 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-result 0.4.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-strings 0.4.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-strings 0.5.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-sys 0.48.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-sys 0.59.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-sys 0.61.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-targets 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-targets 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-threading 0.1.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-threading 0.2.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows-version 0.1.7 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_aarch64_gnullvm 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_aarch64_gnullvm 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_aarch64_msvc 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_aarch64_msvc 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_i686_gnu 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_i686_gnu 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_i686_gnullvm 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_i686_msvc 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_i686_msvc 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_gnu 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_gnu 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_gnullvm 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_gnullvm 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_msvc 0.48.5 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED windows_x86_64_msvc 0.52.6 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winnow 0.7.15 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winreg 0.55.0 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winresource 0.1.32 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED winsafe 0.0.19 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wio 0.2.2 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED wit-bindgen 0.57.1 registry+https://github.com/rust-lang/crates.io-index
LICENSE_UNVERIFIED xml-rs 0.8.29 registry+https://github.com/rust-lang/crates.io-index
LICENSES lock_dependencies=697 inventory=510 collected=510 unverified=187 status=2
SECRET_CANDIDATE worktree "src/rpc.rs" line=735 openai-anthropic-token
SECRET_CANDIDATE worktree "tests/diagnostic_privacy_smoke.py" line=11 openai-anthropic-token
SECRET_CANDIDATE worktree "tests/fixtures/fake-pi-sdk/dist/index.js" line=18 openai-anthropic-token
SECRET_CANDIDATE worktree "tests/fixtures/fake_pi.py" line=23 openai-anthropic-token
SECRET_CANDIDATE 323981096553ed89c60b9fb35fa0813be55391f8 "diff --git a/src/rpc.rs b/src/rpc.rs" diff_line=148 openai-anthropic-token
SECRET_CANDIDATE 323981096553ed89c60b9fb35fa0813be55391f8 "diff --git a/tests/diagnostic_privacy_smoke.py b/tests/diagnostic_privacy_smoke.py" diff_line=11 openai-anthropic-token
SECRET_CANDIDATE 323981096553ed89c60b9fb35fa0813be55391f8 "diff --git a/tests/fixtures/fake_pi.py b/tests/fixtures/fake_pi.py" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE da759e09314a12812e56f06dba3509fa477c1814 "diff --git a/src/rpc.rs b/src/rpc.rs" diff_line=148 openai-anthropic-token
SECRET_CANDIDATE da759e09314a12812e56f06dba3509fa477c1814 "diff --git a/tests/diagnostic_privacy_smoke.py b/tests/diagnostic_privacy_smoke.py" diff_line=11 openai-anthropic-token
SECRET_CANDIDATE da759e09314a12812e56f06dba3509fa477c1814 "diff --git a/tests/fixtures/fake_pi.py b/tests/fixtures/fake_pi.py" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE a3665d27a2a8cba8e3f2972efbfff2ab5c509cc6 "diff --git a/src/rpc.rs b/src/rpc.rs" diff_line=148 openai-anthropic-token
SECRET_CANDIDATE a3665d27a2a8cba8e3f2972efbfff2ab5c509cc6 "diff --git a/tests/diagnostic_privacy_smoke.py b/tests/diagnostic_privacy_smoke.py" diff_line=11 openai-anthropic-token
SECRET_CANDIDATE a3665d27a2a8cba8e3f2972efbfff2ab5c509cc6 "diff --git a/tests/fixtures/fake_pi.py b/tests/fixtures/fake_pi.py" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE 11c6da4f2a67b9eae773e51bf6e205398e18b9f5 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=16 openai-anthropic-token
SECRET_CANDIDATE 128d7fbd68b35c0d5bcf34bb8026c333665ba6a3 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=16 openai-anthropic-token
SECRET_CANDIDATE aa7e1c3ee77acf55bf481a9c08ccea7335cddb33 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=16 openai-anthropic-token
SECRET_CANDIDATE 7a0505e3a60efacad3294b4a086edccc0642b3b1 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE ad7697f9f66fc740e540991c492bbf683db912a4 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=16 openai-anthropic-token
SECRET_CANDIDATE 4e8ee9830848d9c4dd359e91e3fa3967208d842e "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE 9f9944100d5d97db44a5ab8d32c96d5f34550a6f "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE c80f97ac562191ec6618e0c12ae729cd090bb531 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=7 openai-anthropic-token
SECRET_CANDIDATE 55c4c52b120812661a5ae00a1e7371635c8f1452 "diff --git a/tests/fixtures/fake-pi-sdk/dist/index.js b/tests/fixtures/fake-pi-sdk/dist/index.js" diff_line=7 openai-anthropic-token
SECRETS tracked_files=148 binary_files_skipped=3 commits=195 candidates=22 shallow=False
AUDIT_EXIT 3 (0=clean, 1=findings, 2=incomplete, 3=both)
```

## Testes — rodada 2

Executados com rede desativada para Cargo (`CARGO_NET_OFFLINE=true`), target
local gravável e sem alterar Cargo.toml/Cargo.lock:

```sh
TMPDIR=/tmp cargo test --locked
python3 -m unittest tests.test_security_audit -v
bash -n scripts/security-audit.sh
git diff --check
git diff --exit-code -- Cargo.toml Cargo.lock
```

Saída integral Rust (exit 0):

```text
   Compiling dish v0.1.0 (/home/cachy/dev/dish-issue90)
    Finished `test` profile [optimized + debuginfo] target(s) in 25.52s
     Running unittests src/main.rs (target/debug/deps/dish-4c3169b960064bf3)

running 211 tests
test content_search::tests::benchmark_large_history ... ignored, manual performance measurement; no machine-dependent timing assertion
test content_search::tests::cancellation_and_empty_query_return_no_hits ... ok
test content_search::tests::highlight_offsets_preserve_original_utf8_boundaries ... ok
test content_search::tests::indexes_tools_but_not_images ... ok
test content_search::tests::unreadable_file_is_reported_without_hiding_other_results ... ok
test content_search::tests::unchanged_files_are_cached_and_modified_files_reloaded ... ok
test demand::tests::arguments_isolate_pin_the_model_and_carry_only_the_demand ... ok
test content_search::tests::searches_live_unicode_and_caps_results ... ok
test demand::tests::normalize_collapses_whitespace_and_clip_bounds_length ... ok
test demand::tests::real_demands_are_substantive ... ok
test demand::tests::sanitize_accepts_a_clean_line_and_strips_decoration ... ok
test content_search::tests::snippet_handles_unicode_case_expansion_and_long_lines ... ok
test demand::tests::sanitize_bounds_the_title_length ... ok
test demand::tests::sanitize_rejects_explanations_and_empty_output ... ok
test demand::tests::short_follow_ups_are_not_substantive ... ok
test demand::tests::structured_output_ignores_thinking_and_rejects_failed_messages ... ok
test demand::tests::summary_overrides_original_and_stays_visible_between_requests ... ok
test editor::tests::history_ignores_blank_and_consecutive_duplicates ... ok
test editor::tests::history_walks_backwards_and_restores_the_draft ... ok
test editor::tests::horizontal_move_collapses_selection_in_direction_of_travel ... ok
test editor::tests::horizontal_move_keeps_multibyte_offsets_on_grapheme_boundaries ... ok
test editor::tests::horizontal_move_steps_one_grapheme_with_empty_selection ... ok
test editor::tests::horizontal_move_with_extend_steps_from_the_head ... ok
test editor::tests::selection_clamps_to_utf8_boundaries_and_buffer_length ... ok
test editor::tests::sending_after_recall_starts_a_new_draft_cycle ... ok
test editor::tests::stale_selection_on_empty_buffer_can_be_replaced ... ok
test editor::tests::undo_budget_drops_oldest_snapshots_but_keeps_the_latest ... ok
test fs_security::tests::private_directory_rejects_a_symlink_at_the_final_component ... ok
test fs_security::tests::write_replaces_a_file_and_creates_a_private_directory ... ok
test fs_security::tests::write_replaces_a_symlink_without_touching_its_target ... ok
test git_context::tests::numstat_handles_binary_renames_and_unusual_names ... ok
test git_context::tests::parses_unborn_detached_divergence_and_renames_without_counting_paths ... ok
test git_context::tests::summaries_are_compact_and_non_repo_states_are_explicit ... ok
test installation::tests::data_root_follows_the_platform_convention ... ok
test installation::tests::explicit_program_without_a_package_still_names_the_candidate ... ok
test installation::tests::override_wins_and_private_install_is_found_without_path_changes ... ok
test installation::tests::discovered_program_passes_an_existing_package ... ok
test installation::tests::windows_install_locations_are_searched_without_path ... ok
test limits::tests::an_oversized_line_never_returns_its_contents ... ok
test limits::tests::draining_skips_the_whole_oversized_line ... ok
test limits::tests::limits_count_bytes_not_characters ... ok
test limits::tests::reads_lines_under_the_limit_and_reports_eof ... ok
dish: teste 1
test logging::tests::warning_never_panics ... ok
test markdown::tests::image_previews_stay_inside_the_project_and_reject_remote_sources ... ok
test markdown::tests::inline_markup_still_styles_after_the_linear_scan ... ok
test markdown::tests::malformed_inline_brackets_do_not_rescan_or_hide_later_markup ... ok
test markdown::tests::standalone_images_parse_without_consuming_partial_streams ... ok
test markdown::tests::table_cells_preserve_inline_code_and_escaped_pipes ... ok
test markdown::tests::tables_require_matching_header_and_delimiter_columns ... ok
test git_context::tests::handles_missing_git_and_non_repo ... ok
test metadata::tests::local_format_and_provenance_are_explicit ... ok
test metadata::tests::missing_or_invalid_history_never_gets_now_or_duration ... ok
test metadata::tests::response_end_is_not_task_success_and_abort_is_not_error ... ok
test installation::tests::external_terminal_receives_fixed_installer_arguments ... ok
test metadata::tests::wall_clock_changes_do_not_corrupt_duration_and_end_is_idempotent ... ok
test model_sync::tests::a_background_refresh_does_not_hide_a_switch_failure ... ok
test model_sync::tests::a_failed_latest_switch_reloads_the_actual_model_after_an_intermediate_success ... ok
test model_sync::tests::a_new_choice_discards_an_old_state_and_serializes_switches ... ok
test model_sync::tests::a_superseded_failure_does_not_fail_the_latest_choice ... ok
test model_sync::tests::failure_never_marks_effort_ready_and_a_refresh_retries ... ok
test model_sync::tests::level_events_can_refresh_a_pending_snapshot_without_erasing_a_choice ... ok
test model_sync::tests::rapid_changes_coalesce_and_stale_levels_are_discarded ... ok
test model_sync::tests::switch_waits_for_confirmation_then_state_then_levels ... ok
test platform::tests::hiding_the_console_is_safe_on_any_platform ... ok
test preferences::tests::config_root_follows_the_platform_convention ... ok
test preferences::tests::atomic_write_creates_directory_and_replaces_file ... ok
test preferences::tests::motion_preference_preserves_explicit_choices ... ok
test preferences::tests::old_or_partial_preferences_use_defaults ... ok
test preferences::tests::preferences_round_trip ... ok
test preferences::tests::remember_project_moves_to_front_dedupes_and_caps ... ok
test installation::tests::rejects_installer_success_without_a_working_pi ... ok
test installation::tests::transient_exec_busy_is_retried_but_other_errors_are_not ... ok
test process::tests::captures_bounded_output_and_status ... ok
test process::tests::output_over_the_limit_is_an_error ... ok
test provider_login::tests::a_secret_question_is_parsed_without_any_value ... ok
test provider_login::tests::a_select_question_carries_its_options ... ok
test provider_login::tests::a_successful_login_reports_provider_and_type_only ... ok
test provider_login::tests::failures_become_portuguese_sentences_without_the_raw_detail ... ok
test provider_login::tests::oauth_notices_arrive_in_their_own_shape ... ok
test provider_login::tests::unknown_and_malformed_lines_are_ignored ... ok
test installation::tests::detects_missing_failed_and_invalid_executables ... ok
test installation::tests::installs_with_fake_npm_and_validates_result ... ok
test providers::tests::adapter_command_carries_the_pi_location ... ok
test providers::tests::an_empty_catalog_is_the_first_use_case ... ok
test providers::tests::parse_accepts_unknown_sources_and_missing_fields ... ok
test providers::tests::parse_keeps_only_the_contract_fields ... ok
test providers::tests::parse_maps_errors_to_portuguese_without_the_raw_detail ... ok
test providers::tests::parse_rejects_garbage ... ok
test providers::tests::the_embedded_adapter_is_the_script ... ok
test rpc::privacy_tests::command_encoding_stops_at_the_limit ... ok
test rpc::privacy_tests::extension_ui_reply_preserves_request_id_and_command_ids_remain_fresh ... ok
test rpc::privacy_tests::malformed_record_summary_omits_the_payload ... ok
test rpc::privacy_tests::stderr_diagnostics_never_retain_the_line ... ok
test rpc::privacy_tests::stderr_error_detection_is_case_insensitive ... ok
test git_context::tests::terminates_hanging_command ... ok
test git_context::tests::pure_rename_does_not_inflate_line_counts ... ok
test installation::tests::checks_node_and_npm_prerequisites ... ok
test providers::tests::a_non_zero_exit_still_returns_the_body ... ok
test git_context::tests::line_counts_are_net_head_changes_not_staged_plus_unstaged_sum ... ok
test session_activity::tests::aborted_error_message_is_not_a_failure ... ok
test session_activity::tests::completed_reads_as_done_instead_of_idle ... ok
test session_activity::tests::filters_are_independent ... ok
test session_activity::tests::only_completed_live_text_creates_unread_content ... ok
test session_activity::tests::relative_time_handles_missing_future_and_old_activity ... ok
test session_activity::tests::saved_and_interrupted_are_not_running_or_errors ... ok
test session_activity::tests::tool_output_counts_but_a_tool_call_alone_does_not ... ok
test sessions::tests::a_short_follow_up_does_not_rename_the_session ... ok
test sessions::tests::a_partial_last_line_is_reprocessed_after_append ... ok
test sessions::tests::an_unrepresentable_path_is_not_moved_without_metadata ... ok
test sessions::tests::an_atomic_replacement_with_preserved_mtime_is_rescanned ... ok
test sessions::tests::append_resumes_at_the_last_complete_line ... ok
test sessions::tests::discovers_and_groups_by_header_not_directory_name ... ok
test sessions::tests::no_clobber_move_preserves_both_files_on_conflict ... ok
test sessions::tests::distinguishes_same_named_projects_and_caches_unchanged_files ... ok
test sessions::tests::refuses_to_trash_a_non_session_file ... ok
test sessions::tests::replacing_a_session_with_the_same_length_rescans_it ... ok
test sessions::tests::restoring_never_overwrites_a_recreated_session ... ok
test sessions::tests::strips_original_session_selection ... ok
test sessions::tests::unnamed_session_title_follows_the_latest_demand ... ok
test sessions::tests::trashes_and_restores_a_session ... ok
test settings::model_grouping_tests::group_state_reports_on_off_and_mixed ... ok
test settings::model_grouping_tests::groups_by_provider_and_sorts_models_alphabetically ... ok
test state::effort_tests::attachment_budget_counts_decoded_payload_size ... ok
test state::effort_tests::cycles_only_supported_levels_and_wraps ... ok
test state::effort_tests::file_uris_are_decoded_without_reinterpreting_normal_text ... ok
test state::effort_tests::handles_empty_and_single_level_lists ... ok
test state::effort_tests::image_clipboard_uses_pi_image_content_contract ... ok
test state::effort_tests::pi_off_only_ladder_means_no_reasoning ... ok
test state::effort_tests::reconcile_effort_keeps_valid_and_falls_back_deterministically ... ok
test state::effort_tests::scrolling_up_disengages_tail_even_with_unmeasured_messages ... ok
test state::metadata_tests::absent_pi_timestamp_does_not_relabel_a_local_echo ... ok
test state::metadata_tests::restored_tool_result_keeps_pi_time_without_fake_execution_times ... ok
test state::title_tests::a_short_follow_up_keeps_the_previous_demand ... ok
test state::title_tests::an_empty_transcript_has_no_title ... ok
test state::title_tests::latest_substantive_demand_wins_over_the_first ... ok
test state::title_tests::only_short_messages_fall_back_to_the_latest ... ok
test state::turn_end_tests::an_error_already_rendered_in_the_transcript_is_not_duplicated ... ok
test state::turn_end_tests::interruption_keeps_the_warning_classification ... ok
test state::turn_end_tests::only_an_explicit_error_message_produces_a_banner_or_note ... ok
test tests::path_like_arguments_are_recognized ... ok
test theme::tests::appearance_mode_decides_the_palette ... ok
test theme::tests::appearance_mode_round_trips_through_json ... ok
test theme::tests::attention_reads_as_new_content_not_as_running_or_warning ... ok
test theme::tests::built_in_catalog_is_unique_and_ordered ... ok
test theme::tests::delta_e_anchors_are_sane ... ok
test theme::tests::effort_colors_match_pi_dark_theme ... ok
test theme::tests::git_diff_colors_are_readable_and_distinguishable ... ok
test theme::tests::directory_scan_orders_filters_and_reports ... ok
test theme::tests::hex_parser_accepts_six_or_eight_digits_only ... ok
test theme::tests::light_effort_ramp_keeps_the_dark_hues_and_stays_readable ... ok
test theme::tests::status_colors_are_distinct_from_the_effort_ramp ... ok
test theme::tests::malformed_user_themes_report_the_file ... ok
test theme::tests::success_neutral_stays_off_the_neutral_effort ... ok
test theme::tests::text_meets_contrast_requirement ... ok
test theme::tests::user_theme_defaults_name_and_appearance ... ok
test theme::tests::user_theme_file_loads_with_its_colors ... ok
test git_context::tests::unborn_line_counts_compare_worktree_to_empty_tree_without_writing_it ... ok
test ui::composer::tests::keyboard_effort_uses_pi_capabilities_and_stops_at_edges ... ok
test ui::inspector::tests::details_keep_the_original_desktop_breakpoint ... ok
test ui::inspector::tests::long_paths_keep_their_tail_and_cut_at_a_separator ... ok
test ui::motion::tests::motion_responds_without_a_long_entrance ... ok
test ui::motion::tests::transition_is_bounded_monotonic_and_finishes_exactly ... ok
test ui::overlays::model_search_tests::hidden_models_leave_the_picker_but_never_the_active_one ... ok
test ui::overlays::model_search_tests::hiding_everything_falls_back_to_showing_all ... ok
test ui::overlays::model_search_tests::matches_model_and_provider_case_insensitively ... ok
test ui::overlays::model_search_tests::search_and_visibility_compose ... ok
test ui::selectable::tests::blocks_share_plain_text_offsets ... ok
test ui::selectable::tests::search_spans_preserve_markdown_runs_and_total_length ... ok
test ui::selectable::tests::selection_highlight_preserves_runs ... ok
test ui::selectable::tests::unicode_word_selection ... ok
test ui::test_probe::tests::normal_sessions_do_not_evaluate_probe_payloads ... ok
test ui::transcript::copy_tests::copies_exact_multiline_command_not_summary ... ok
test ui::transcript::copy_tests::preserves_partial_arguments_and_serializes_complete_arguments ... ok
test ui::transcript::copy_tests::tool_metadata_key_is_stable_across_a_block_reindex ... ok
test ui::transcript::copy_tests::unstarted_or_cancelled_tools_are_not_reported_as_success ... ok
test workspace::search::tests::editing_query_keeps_complete_list_until_replacement_is_ready ... ok
test workspace::search::tests::changed_scope_clears_old_results_even_when_query_is_identical ... ok
test ui::window_frame::tests::client_frame_follows_decorations_and_windows_workaround ... ok
test workspace::search::tests::empty_query_clears_list_and_disappearing_selection_is_not_retargeted ... ok
test workspace::search::tests::live_snapshots_are_reused_until_the_transcript_revision_changes ... ok
test workspace::search::tests::repeated_renders_and_editor_notifications_keep_snapshot_and_selection ... ok
test workspace::tests::attention_counts_are_compact_and_do_not_hide_real_counts ... ok
test workspace::tests::attention_signals_do_not_imply_running ... ok
test workspace::tests::catalog_snapshot_fences_paths_while_shutdown_and_trash_are_pending ... ok
test workspace::tests::delayed_live_action_keeps_identity_when_an_earlier_session_closes ... ok
test workspace::tests::deleting_a_session_removes_it_from_the_catalog ... ok
test workspace::tests::filter_counts_include_attention_without_implying_running ... ok
test workspace::tests::lifecycle_distinguishes_current_open_and_saved_without_activity ... ok
test workspace::tests::nav_highlight_follows_the_selected_entry ... ok
test workspace::tests::opening_saved_session_preserves_its_position_during_interaction ... ok
test workspace::tests::row_order_has_stable_ties_and_freezes_during_interaction ... ok
test workspace::tests::running_filter_excludes_open_but_inactive_sessions ... ok
test workspace::tests::saved_action_resolves_a_file_that_became_live_after_menu_opened ... ok
test workspace::tests::selecting_empty_session_list_is_safe ... ok
test workspace::tests::selecting_removed_last_session_preserves_active_session ... ok
test workspace::tests::selecting_valid_session_updates_active_session ... ok
test workspace::tests::stale_catalog_snapshot_cannot_resurrect_a_trashed_session ... ok
test workspace::tests::undoing_a_delete_reinserts_the_session_in_order ... ok
test installation::tests::hanging_command_is_terminated ... ok
test markdown::tests::wide_table_vertical_wheel_scrolls_transcript_and_shift_wheel_scrolls_table ... ok
test rpc::tests::stderr_is_summarized_before_it_reaches_the_ui ... ok
test git_context::tests::divergence_uses_only_local_upstream_refs ... ok
test providers::tests::a_hanging_probe_is_terminated ... ok
test process::tests::closing_stdout_does_not_hide_a_running_child ... ok
test git_context::tests::discovers_real_repo_subfolder_worktree_and_detached_head_without_writing_index ... ok
test process::tests::timeout_covers_a_child_that_keeps_running ... ok
test rpc::shutdown_tests::explicit_shutdown_waits_for_file_writer_and_fences_retained_clones ... ok
test demand::tests::isolated_process_reports_failures_and_bounds_exit_wait ... ok
test rpc::tests::a_writer_failure_after_acceptance_is_reported ... ok
test process::tests::timeout_covers_a_descendant_that_keeps_the_pipe_open ... ok
test rpc::tests::independent_processes_keep_streaming_and_isolate_commands ... ok

test result: ok. 210 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1.09s

```

Saída integral Python (exit 0):

```text
test_binary_is_excluded (tests.test_security_audit.SecurityAuditTests.test_binary_is_excluded) ... ok
test_common_secret_formats_and_redaction (tests.test_security_audit.SecurityAuditTests.test_common_secret_formats_and_redaction) ... ok
test_coordinator_installation_is_in_detection_path (tests.test_security_audit.SecurityAuditTests.test_coordinator_installation_is_in_detection_path) ... ok
test_cvss_severity_and_unknown_vectors (tests.test_security_audit.SecurityAuditTests.test_cvss_severity_and_unknown_vectors) ... ok
test_deleted_secret_on_other_branch_is_detected_without_disclosure (tests.test_security_audit.SecurityAuditTests.test_deleted_secret_on_other_branch_is_detected_without_disclosure) ... ok
test_license_choices_and_obligations (tests.test_security_audit.SecurityAuditTests.test_license_choices_and_obligations) ... ok
test_local_auditor_uses_offline_database_and_preserves_status (tests.test_security_audit.SecurityAuditTests.test_local_auditor_uses_offline_database_and_preserves_status) ... ok
test_missing_auditor_is_incomplete_not_clean (tests.test_security_audit.SecurityAuditTests.test_missing_auditor_is_incomplete_not_clean) ... ok
test_tool_failure_is_not_zero_vulnerabilities (tests.test_security_audit.SecurityAuditTests.test_tool_failure_is_not_zero_vulnerabilities) ... ok

----------------------------------------------------------------------
Ran 9 tests in 0.039s

OK
```

Os sete testes anteriores foram preservados; dois testes novos cobrem detecção
no caminho do coordenador, override do binário/DB, flags sem rede e códigos
0/1/2/3. O único teste Rust ignorado já é marcado como benchmark manual no
código original; não foram removidos, pulados ou afrouxados testes nesta rodada.
A suíte de empacotamento não foi reexecutada nesta rodada: a inspeção foi somente
leitura, sem instalação. Na rodada anterior ela tinha 1 falha/1 erro por falta
de ferramentas Debian e 4 skips condicionais existentes; não se afirma que passou.

## Pendências e texto proposto para a issue (não enviado)

> Rodada 2 sobre e6bc9f8, 2026-10-25: RustSec confirmou 0 vulnerabilidades
> (críticas/altas/médias/baixas = 0), 1 unsound e 5 unmaintained, todos triados
> em docs/security-audit.md. Tokio/AsyncFd não está no grafo Linux nem em src/;
> aceitação restrita a esse escopo, com recomendação de atualização futura.
> Tar e Debian reais tiveram checksums, conteúdo e licenças inspecionados;
> BUILD.json do tar confere, mas está ausente no Debian. Rust: 210 aprovados,
> 1 ignored original; Python: 9 aprovados. Auditoria geral retorna 3: avisos e
> 22 candidatos sintéticos preservados, mais 187/697 licenças não verificadas.
> Manter fechamento pendente até completar cobertura multiplataforma e tratar
> as limitações registradas; nenhuma alteração da issue foi feita.

Continuam pendentes a cobertura das 187 licenças fora do inventário Linux,
a resolução/testes de futuras migrações, análise de outros targets/features,
consulta de crates yanked e verificação externa de proveniência da publicação.
Sem rede, não foram atualizados o DB, refs remotas nem ferramentas. Os limites
do scanner de segredos continuam válidos. A ausência de BUILD.json no Debian
fica registrada para melhoria do empacotador. Não houve commit, push ou mudança
em Cargo.toml/Cargo.lock; os quatro arquivos da entrega continuam não versionados.
