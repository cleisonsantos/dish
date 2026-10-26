# Atualização do Dish no lugar (#57)

**Status:** design consolidado; nenhuma implementação nesta mudança.

**Base verificada:** `origin/main` em `e6bc9f8` (2026-10-25).

**Decisão em uma frase:** primeiro notificar com um manifesto assinado; depois
atualizar automaticamente **somente** a instalação portátil em
`~/.local/bin/dish`; a instalação `.deb` continua sendo atualizada pelo
gerenciador de pacotes e a decisão entre repositório APT e AppImage fica para a
Fase 3.

Este documento registra o que o Dish faz hoje, as opções consideradas, a
decisão recomendada, o contrato do manifesto, o modelo de confiança e o
fatiamento em PRs pequenos. Ele foi escrito para ser implementado em partes e
não altera código nesta mudança; um cliente HTTP só entra no PR da Fase 1 que o
necessitar.

## 1. O que existe hoje (verificado)

### 1.1 Versão do aplicativo

- A versão-base vive em `Cargo.toml:3` e é `0.1.0`.
- A tela **Configurações → Sobre / Pi** mostra
  `env!("CARGO_PKG_VERSION")` (`src/settings.rs:1226`), portanto mostra
  `0.1.0`, não o rótulo do release.
- Não existe a opção `--version`. O parser de argumentos entrega opções iniciais
  ao Pi (`src/main.rs:188-211`); `dish --version` abriria a janela e passaria
  `--version` ao processo do Pi.
- O rótulo `0.1.0-alpha.5` aparece no argumento do empacotador, no nome do
  arquivo e no `BUILD.json` (`scripts/package-linux.py:84-87`, `93`,
  `113-115`), mas não entra no binário.

**Consequência crítica:** se o Dish comparar `0.1.0` com `0.1.0-alpha.5`, a
regra SemVer diz que `0.1.0` é **maior** que o pré-release. Nenhuma atualização
seria oferecida. A implementação precisa embutir o rótulo do release no binário
antes de qualquer comparação de versão (Seção 3.1).

### 1.2 Rede e releases

- Não há cliente HTTP nas dependências (`Cargo.toml:8-21`). O uso de rede do
  aplicativo é abrir URLs no navegador (`src/platform.rs:43-47`) e executar o
  instalador oficial do Pi via `curl` num terminal
  (`src/installation.rs:231`, `396-398`).
- Os releases publicam os assets abaixo. Valores reais de `v0.1.0-alpha.5`:

  | Asset | Tamanho | SHA-256 |
  | --- | ---: | --- |
  | `dish-0.1.0-alpha.5-linux-x86_64.tar.gz` | 17.140.383 | `89d5f283…d4742524` |
  | `dish-pi_0.1.0-alpha.5_amd64.deb` | 13.433.234 | `827b846f…4f4f8185` |
  | `dish-0.1.0-alpha.5-windows-x86_64-setup.exe` | 5.683.917 | (fora do escopo Linux) |

  Cada asset tem um `.sha256` ao lado. O Windows é publicado por
  `.github/workflows/windows.yml:109-149`; o workflow Linux
  (`.github/workflows/ci.yml:15-16`) tem permissões somente de leitura e **não**
  publica um release.
- Os cinco releases existentes (`v0.1.0-alpha.1` a `v0.1.0-alpha.5`) são
  **prerelease**. O endpoint `GET /repos/cleisonsantos/dish/releases/latest`
  respondeu **404** em 2026-10-25; `GET /releases` lista normalmente os cinco.
  O endpoint “latest” exclui prereleases por definição.
- O `BUILD.json` do arquivo portátil contém, no alpha.5:
  `version`, `target`, `minimum_glibc`, `dependency_packages`, `rustc`,
  `cargo_lock_sha256` e `icon_status` (`scripts/package-linux.py:113-115`).
  Ele **não** é instalado pelo `.deb` (`scripts/package-deb.py:178-200`) nem
  copiado pelo `install.sh` (`scripts/install-release.sh:30-49`). O aplicativo
  em execução não pode lê-lo para descobrir a própria versão.

### 1.3 Formas de instalação

O texto da issue fala em `/usr/bin/dish`; há uma correção importante: desde o
alpha.4 o `.deb` instala `/usr/bin/dish-pi`, para não disputar o pacote `dish`
do Debian. O desenho cobre as duas gerações.

| Forma | Executável | Dono / gerência | Atualização |
| --- | --- | --- | --- |
| Arquivo portátil, `install.sh` | `~/.local/bin/dish` | usuário, sem sudo | Fase 2 pode substituir |
| `.deb` atual (`dish-pi`) | `/usr/bin/dish-pi` | root, apt/dpkg | notificar; atualizar pelo apt |
| `.deb` antigo (até alpha.3) | `/usr/bin/dish` | root, apt/dpkg | notificar e orientar migração |
| Arquivo extraído sem instalar | `./bin/dish` | usuário | manual; nunca auto-atualizar |

Evidências: `scripts/install-release.sh:34-44` grava o binário, o ícone, o
desktop entry e as licenças; `scripts/package-deb.py:30` e `178-184` definem o
pacote `dish-pi` e `/usr/bin/dish-pi`; `docs/linux-release.md:37-54` documenta a
migração do nome antigo.

### 1.4 Peças reutilizáveis (e seus limites)

- `retry_text_busy` faz um retry limitado de `ETXTBSY`
  (`src/installation.rs:293-312`). Hoje ele envolve **spawn** de processos
  (`src/installation.rs:318-326`), não substituição de arquivo. Não é o
  mecanismo de troca do binário.
- `write_atomic` cria um temporário com `create_new`, faz `fsync` e `rename`
  (`src/fs_security.rs:40-73`); as preferências usam esse caminho por um único
  escritor (`src/preferences.rs:88-109`). É o modelo para estado, mas o binário
  precisa de modo `0755`, não `0600`.
- `last_project` e `last_session` são gravados continuamente em
  `Workspace::render` (`src/workspace.rs:2124-2141`). Sem argumentos, o
  aplicativo restaura esses valores (`src/main.rs:192`, `226-233`). Isso permite
  reiniciar sem repetir o prompt da linha de comando.
- `AppState::is_busy` (`src/state.rs:2773-2775`) já distingue execução em
  andamento e pode bloquear um reinício forçado.
- Existe um banner de conversa (`src/ui/mod.rs:66-68`, `599-650`), mas ele é
  local a uma conversa e só tem texto e “dispensar”. O aviso de atualização é
  global e precisa de ações (“Notas”, “Baixar”, “Atualizar”), por isso nasce no
  `Workspace`, não no `AppState`.

## 2. Opções consideradas

| Opção | O que é | Decisão |
| --- | --- | --- |
| **A. Só notificar** | Consultar o release, comparar versão e mostrar banner com notas/download. Nenhuma escrita, nenhum privilégio. | **Fase 1**, com manifesto assinado. |
| **B. Self-update portátil** | Baixar o `.tar.gz`, verificar, extrair ao lado do destino, `rename` e manter o binário anterior. Só para `~/.local/bin/dish`. | **Fase 2**, condicionado aos critérios da Seção 3.2. |
| **C. Atualizar `.deb`** | Rodar `pkexec`/`apt` para o `.deb`, ou só abrir a página do release. | Notificar e abrir a página; **nunca** escalar privilégio dentro do Dish. |
| **D. Repositório APT** | `InRelease` assinado em GitHub Pages, `sources.list.d` + chaveiro, `apt upgrade` resolve tudo. | **Fase 3**, depois de medir a base `.deb`. |
| **E. Ferramentas externas / AppImage** | `topgrade`, `deb-get`, `AppImageUpdate`. | Não agora; AppImage é candidato da Fase 3. |

Também foram descartadas ou adiadas:

- **Usar apenas `/releases/latest`:** hoje retorna 404, porque todos os releases
  são prerelease. A descoberta precisa listar releases e escolher por SemVer.
- **Shell out para `curl`:** criaria dependência de um executável externo, sem
  controle tipado de timeout, redirecionamento, allowlist e erros; também
  dificulta os testes sem rede. Um cliente HTTP pequeno é mais auditável.
- **Baixar e executar `pkexec`/`apt`:** interativo, depende de polkit e mistura
  o Dish com o gerenciador de pacotes. O caminho correto para `/usr/bin` é o apt.
- **Servidor próprio de atualização:** GitHub Releases continua sendo o canal;
  um servidor adicionaria operação e outro domínio de confiança.
- **Checagem frequente (a cada hora):** desnecessária e hostil com o rate limit
  anônimo do GitHub. O limite é 1 vez por dia.
- **Downgrade automático:** nunca. Trocar de canal não rebaixa uma versão.

## 3. Desenho recomendado

### 3.1 Fase 1 — notificar (sem escrever binário)

Escopo:

1. Embutir o rótulo do release no binário e expor `--version`.
2. Publicar `latest.json` + `latest.json.minisig` em cada release.
3. Consultar no máximo uma vez por dia, fora da thread da UI, e comparar versões.
4. Mostrar um banner global dispensável com “Notas” e “Baixar”, e um botão
   **Verificar atualizações** em **Configurações → Sobre / Pi**.
5. Não baixar, extrair, substituir nem executar binário; não pedir root.

Decisão de versão: `DISH_RELEASE_VERSION` é definido pelo empacotador no
momento do `cargo build` e lido com `option_env!`. Quando ausente, o binário é
**build local** (`0.1.0 (build local)`); ele nunca procura atualização
automática. O `About` passa a mostrar o rótulo completo; `--version` imprime uma
linha estável, por exemplo `dish 0.1.0-alpha.5`, antes de inicializar o GPUI e
antes de checar o Pi. O `BUILD.json` continua sendo a autoridade do arquivo; o
teste do arquivo compara `BUILD.json.version` com a saída de `bin/dish
--version`.

Assinatura já na Fase 1: o manifesto é assinado desde o primeiro release que o
cliente consome. Isso evita que uma conta GitHub comprometida injete um banner
ou uma URL de notas falsa e evita uma migração de confiança na Fase 2. A
Fase 1 não substitui nenhum binário; a assinatura protege a **informação**.

**Aceite da Fase 1**

- `dish --version` não abre janela e imprime o rótulo completo; um build local
  imprime a marca de desenvolvimento.
- O release contém `latest.json` e `latest.json.minisig`; a assinatura verifica
  com a chave pública embutida no binário.
- Uma consulta automática só ocorre se `now - last_update_check >= 24 h`; a
  consulta roda fora da thread da UI e não bloqueia a abertura.
- Manifesto ausente, assinatura inválida, versão inválida, release sem os assets
  esperados, rede indisponível ou rate limit: **nenhum banner**, nenhuma
  exceção visível, apenas log; a checagem manual mostra uma mensagem genérica.
- O banner é dispensável por versão e não reaparece para a mesma versão; uma
  versão mais nova o reexibe.
- O banner funciona nas duas formas de instalação; no `.deb`, a ação é “Baixar”
  e abre a página do release, sem `pkexec`.
- Nenhuma escrita fora de `~/.config/dish/state.json` (preferências), nenhuma
  alteração em `~/.local/bin`, `/usr/bin` ou arquivos do Pi.
- Testes unitários de versão, manifesto, canal, cadência e forma de instalação;
  teste de integração com servidor HTTP local; testes de release verificam que
  cada entrada do manifesto corresponde ao asset real.

### 3.2 Fase 2 — self-update assinado da instalação portátil

Só executa quando **todas** as condições forem verdadeiras:

- `std::env::current_exe()` resolve exatamente para
  `$HOME/.local/bin/dish` (comparação canônica), o arquivo é regular (não é
  symlink) e o diretório é gravável pelo usuário efetivo;
- `$HOME` é absoluto, `$XDG_DATA_HOME` (quando definido) é absoluto e o processo
  não é root;
- a versão candidata é maior que a atual e o canal permite a candidata;
- o arquivo anterior pode ser preservado como `~/.local/bin/dish.old`;
- o lock de atualização (`data_dir()/update.lock`, por padrão
  `~/.local/share/dish/update.lock`) está livre;
- há espaço para o arquivo temporário no mesmo sistema de arquivos do destino.

Em qualquer outro caso — `/usr/bin/dish-pi`, `/usr/bin/dish`, `./bin/dish`,
symlink, diretório somente leitura, `DISH_UPDATE_URL` apontando para um release
incompatível — o comportamento é o da Fase 1: notificar ou nada.

Pipeline:

1. Descobrir o manifesto e verificar a assinatura **antes** de interpretar o
   JSON (Seção 5).
2. Comparar versões e confirmar que a candidata é uma atualização, não um
   downgrade.
3. Baixar o `.tar.gz` para `$XDG_CACHE_HOME/dish/updates/<versão>/`, com limite
   de tamanho, timeout e `sha256` calculado durante o stream. O hash e o tamanho
   precisam bater com o manifesto assinado.
4. Extrair para um diretório temporário privado, recusando caminho absoluto,
   `..` e link simbólico que escape da raiz do arquivo. Exigir um único
   diretório de topo `dish-<versão>-linux-x86_64`.
5. Ler `BUILD.json`: `version` igual ao manifesto e ao nome, `target` igual a
   `x86_64-unknown-linux-gnu`, `minimum_glibc` compatível com a glibc do host.
6. Executar o `install.sh --update` **do próprio arquivo já verificado**, com a
   versão esperada como argumento. O script é o único escritor do payload: ele
   copia `bin/dish` para `~/.local/bin/.dish.<versão>.<pid>.tmp` com
   `create_new` e modo `0755`, faz `fsync`, executa esse temporário com
   `--version` (timeout curto) e confere a saída antes de qualquer troca. O
   diretório de destino é executável por definição; o cache de download não
   precisa ser.
7. O script atualiza licenças, ícone e desktop entry primeiro, usando
   temporário+`rename` em cada arquivo. Falha antes da troca do binário mantém a
   versão atual.
8. Por último, o script cria `dish.old` com hard link (ou cópia com `fsync` se o
   hard link não for possível), troca o destino com `rename` no mesmo diretório
   e faz `fsync` do diretório. Se o `rename` falhar, o destino atual permanece.
9. O Rust confere o código de saída, remove temporários e mantém `dish.old` para
   rollback manual. O manifesto e o arquivo baixado não são reutilizados numa
   tentativa futura sem nova verificação.

`install.sh` ganha um modo `--update` (sem menu, sem prompt) que usa a mesma
lógica de caminhos do instalador manual, mas troca o binário por
temporário+`rename` em vez de escrever por cima do alvo. O atualizador do Rust
executa esse script **somente depois** de verificar assinatura, checksum, versão
e `BUILD.json`. Assim o caminho manual e o caminho automático não divergem, e a
lógica de arquivos fica num só lugar. O script é código do arquivo assinado;
verificá-lo antes de executar é equivalente a verificar o binário. O instalador
já exige `bash` e `python3`; se um deles faltar no momento da atualização, o
script falha sem trocar o binário e o Dish mantém a versão atual. O
`uninstall.sh` também remove `dish.old` e temporários, preservando preferências,
Pi, projetos e sessões.

**`ETXTBSY` e instalação em uso**

O helper de `ETXTBSY` existente cobre o *spawn* de processos, não a troca do
arquivo. A regra da Fase 2 é: nunca abrir o destino para escrita/truncamento;
copiar para um temporário no mesmo diretório e usar `rename(2)`. O processo em
execução continua no inode antigo e o próximo lançamento usa o novo. Se o
`rename` falhar, o Dish mantém a versão atual e reporta o erro; não tenta
“consertar” o binário em uso. Um lock impede duas atualizações concorrentes na
mesma instalação. O `.deb` nunca é tocado.

**Reinício e trabalho não salvo**

Depois da troca, o banner mostra “Atualização pronta” com **Reiniciar agora** e
**Depois**:

- “Depois” não desfaz nada: o binário novo já está no disco e será usado no
  próximo lançamento. O processo atual continua funcionando.
- “Reiniciar agora” só é habilitado se nenhuma conversa estiver executando, se
  não houver rascunho não enviado e se a sessão ativa tiver arquivo de sessão.
  Caso contrário, reaproveita a confirmação de fechamento existente
  (`Workspace::blocking_confirmation`) ou fica desabilitado com explicação.
- O relançamento não reenvia o prompt nem os argumentos originais. Um processo
  auxiliar espera o PID atual sair e inicia o binário com **nenhum argumento**;
  `main.rs` restaura `last_project`/`last_session` como já faz hoje
  (`src/main.rs:192`, `226-233`). Nunca há dois Dish/Pi concorrentes.
- Se o processo auxiliar não conseguir iniciar, o Dish não fecha; mostra o erro.

**Aceite da Fase 2**

- Em `~/.local/bin/dish`, uma atualização válida troca o binário, mantém
  `dish.old`, atualiza licenças/ícone/desktop entry e só então permite reiniciar.
- Checksum errado, tamanho errado, assinatura inválida, versão menor/igual,
  `BUILD.json` divergente, glibc insuficiente, arquivo com caminho perigoso,
  diretório somente leitura, alvo symlink ou lock ocupado **não alteram** o
  binário atual.
- Um binário em execução pode ser substituído sem `ETXTBSY`; o próximo
  lançamento reporta a versão nova. A falha de `rename` mantém a versão velha.
- `/usr/bin/dish-pi` e `/usr/bin/dish` continuam no caminho de notificação; não
  há `sudo`, `pkexec`, `apt` ou escrita em `/usr`.
- O reinício não perde sessão salva; rascunho/execução em andamento bloqueiam o
  botão; não há relançamento automático.
- Testes com arquivo falso, checksum/assinatura inválidos, rollback,
  concorrência, read-only, binário em execução e `uninstall.sh` removendo
  `dish.old`.

### 3.3 Fase 3 — decidir APT vs AppImage

Não implementar enquanto o produto é alpha e glibc-gated. Primeiro medir a base
de instalação sem telemetria:

- contagem pública de downloads de `.deb` e `.tar.gz` pela API de releases;
- relatos em issues/README sobre `apt upgrade`, Debian/Ubuntu e outras
  distribuições;
- custo operacional de manter um repositório APT assinado (chave, `InRelease`,
  GitHub Pages, geração por release) versus um quarto asset AppImage com
  atualização por zsync/`AppImageUpdate`.

Critérios de decisão sugeridos (o dono confirma os números):

- **APT** se a maioria dos usuários Linux estiver em Debian/Ubuntu e pedir
  atualização pelo gerenciador; aceitar a manutenção da chave e do repositório.
- **AppImage** se a portabilidade entre distribuições e o self-update sem root
  forem prioritários; aceitar integração desktop e mais um formato de release.
- **Manter só notificação para `.deb`** se a base for pequena e o custo não se
  pagar; nesse caso não criar infraestrutura só por simetria.

**Aceite da Fase 3:** um documento de decisão com os dados acima, custo
operacional, riscos de chave e o caminho escolhido; nenhum código de atualização
do `.deb` antes da aprovação do dono.

## 4. Manifesto `latest.json`

### 4.1 Onde fica e como é descoberto

Cada GitHub Release publica dois assets adicionais:

- `latest.json` — descrição do release e dos assets instaláveis;
- `latest.json.minisig` — assinatura minisign destacada dos **bytes exatos** do
  manifesto.

O manifesto não lista a si mesmo nem a assinatura em `assets`.

Descoberta padrão, sem token:

```text
GET https://api.github.com/repos/cleisonsantos/dish/releases?per_page=100
Accept: application/vnd.github+json
X-GitHub-Api-Version: <versão documentada, fixada no código>
User-Agent: dish/<versão>
If-None-Match: <etag salvo>
```

O cliente:

1. descarta `draft: true`;
2. filtra pelo canal (`prerelease` aceita pré-release e estável; `stable` só
   estável);
3. lê o `tag_name` `v<versão>`, valida com SemVer e ordena por versão — **não**
   confia na ordem de `created_at` para decidir qual é mais nova;
4. escolhe a maior versão maior que a atual;
5. encontra `latest.json` e `latest.json.minisig` nos assets desse release e
   baixa os dois.

Se o release candidato não tiver manifesto ou assinatura, o cliente falha
fechado e **não** cai silenciosamente para um release anterior. A página do
release continua acessível para download manual.

`DISH_UPDATE_URL` substitui a descoberta e aponta diretamente para um
`latest.json` estático (útil para testes e mirror local). A assinatura é buscada
em `<url>.minisig`; o modo padrão continua sendo GitHub Releases. Não há
servidor próprio.

### 4.2 Schema

`schema` é inteiro e muda só quando o contrato quebra. Campos desconhecidos são
ignorados para permitir evolução; um `schema` maior que 1 é recusado.

| Campo | Tipo | Regra |
| --- | --- | --- |
| `schema` | inteiro | exatamente `1` |
| `version` | string | SemVer 2.0.0, sem `v`, sem build metadata; ex. `0.1.0-alpha.5` |
| `tag` | string | exatamente `v` + `version` |
| `channel` | string | `prerelease` ou `stable`; precisa concordar com `prerelease` do release |
| `published_at` | string | RFC 3339 em UTC; só exibição, nunca decide precedência |
| `notes_url` | string | `https://github.com/cleisonsantos/dish/releases/tag/<tag>` |
| `target` | string | `x86_64-unknown-linux-gnu` |
| `minimum_glibc` | string | `major.minor`, ex. `2.39` |
| `assets` | array | pelo menos um `kind` Linux reconhecido; sem duplicata de `kind` |

Cada item de `assets`:

| Campo | Tipo | Regra |
| --- | --- | --- |
| `kind` | string | `portable-linux-x86_64`, `debian-amd64`; tipos desconhecidos são ignorados |
| `name` | string | nome exato do asset no release |
| `sha256` | string | 64 hex minúsculos; calculado pelo gerador, não copiado do GitHub |
| `size` | inteiro | bytes, maior que zero |
| `url` | string | `https`, host na allowlist da Seção 5 |

O manifesto é pequeno (limite de 64 KiB) e o arquivo portátil tem limite de
download (128 MiB, muito acima dos ~17 MiB atuais).

### 4.3 Exemplo real (alpha.5)

Checksums e tamanhos conferidos contra os assets publicados. O instalador do
Windows foi omitido de propósito: está fora do escopo de atualização desta
issue.

```json
{
  "schema": 1,
  "version": "0.1.0-alpha.5",
  "tag": "v0.1.0-alpha.5",
  "channel": "prerelease",
  "published_at": "2026-10-20T23:50:51Z",
  "notes_url": "https://github.com/cleisonsantos/dish/releases/tag/v0.1.0-alpha.5",
  "target": "x86_64-unknown-linux-gnu",
  "minimum_glibc": "2.39",
  "assets": [
    {
      "kind": "portable-linux-x86_64",
      "name": "dish-0.1.0-alpha.5-linux-x86_64.tar.gz",
      "sha256": "89d5f2834ed7ca193cd738481c1c9061027690125e4bd2dba814c89dd4742524",
      "size": 17140383,
      "url": "https://github.com/cleisonsantos/dish/releases/download/v0.1.0-alpha.5/dish-0.1.0-alpha.5-linux-x86_64.tar.gz"
    },
    {
      "kind": "debian-amd64",
      "name": "dish-pi_0.1.0-alpha.5_amd64.deb",
      "sha256": "827b846f50b7507fdcd5c6019395744e7e86cda4cd4410b956d9d0d65b4f8185",
      "size": 13433234,
      "url": "https://github.com/cleisonsantos/dish/releases/download/v0.1.0-alpha.5/dish-pi_0.1.0-alpha.5_amd64.deb"
    }
  ]
}
```

O gerador (`scripts/package-manifest.py`, futuro) recebe o diretório com os
assets já anexados ao release, **não** reconstrói o binário. Ele calcula os
hashes, confere cada `.sha256` existente, lê `BUILD.json` do arquivo portátil e
só então escreve o JSON de forma determinística (chaves ordenadas, dois espaços,
newline final). A assinatura é feita depois, sobre esses bytes.

### 4.4 Regras de validação

Antes de usar qualquer campo:

1. a assinatura destacada é verificada sobre os bytes crus;
2. o JSON é limitado a 64 KiB e precisa ter `schema: 1`;
3. `tag == "v" + version`, `version` é SemVer válido e `target` é o esperado;
4. `channel` concorda com o `prerelease` do release descoberto;
5. `minimum_glibc` é numérico; `assets` não tem `kind` duplicado nem `name`/`url`
   repetidos;
6. todo `url` é `https`, host permitido, sem credenciais na URL;
7. para o asset portátil, `name` segue
   `dish-<version>-linux-x86_64.tar.gz`; para o `.deb`,
   `dish-pi_<version>_amd64.deb`;
8. `size` e `sha256` são conferidos contra o download real; o `digest` que a API
   do GitHub eventualmente informe é apenas conferência adicional, não
   autoridade.

### 4.5 Comparação de versão

SemVer 2.0.0, com pré-release:

```text
0.1.0-alpha.1  <  0.1.0-alpha.2
0.1.0-alpha.9  <  0.1.0-alpha.10     (identificador numérico)
0.1.0-alpha.1  <  0.1.0-beta
0.1.0-alpha    <  0.1.0-alpha.1      (lista menor vem antes)
0.1.0-alpha.5  <  0.1.0              (pré-release < versão final)
0.1.0+build.1  == 0.1.0+build.2      (build metadata não decide)
```

Regras:

- comparar `major`, `minor`, `patch` numericamente;
- comparar identificadores de pré-release: numérico < alfanumérico, numérico
  por valor, alfanumérico por ASCII, lista menor primeiro quando o prefixo é
  igual;
- ignorar build metadata;
- aceitar apenas a versão exata do release; não “normalizar” `0.1` para
  `0.1.0`;
- nunca atualizar para versão igual ou menor; nunca fazer downgrade automático;
- build local (`DISH_RELEASE_VERSION` ausente ou identificador `local`) não
  entra na comparação automática e não mostra banner.

### 4.6 Canal

- `prerelease` (padrão atual, porque todos os releases são alpha): aceita
  pré-releases e estáveis; a maior versão SemVer vence.
- `stable`: aceita apenas releases sem pré-release.
- O padrão de uma instalação nova é derivado da versão em execução: build
  pré-release continua em `prerelease`; versão estável começa em `stable`. O
  usuário pode mudar em **Sobre / Pi**; a escolha fica em preferências.
- Trocar de canal não rebaixa nada. Um usuário em estável que escolher
  pré-release só recebe uma candidata se ela for SemVer maior.
- O canal não muda a assinatura, o checksum nem a origem dos assets.

### 4.7 Cadência, cache e rate limit

- No máximo **uma consulta automática por 24 horas** por instalação.
- `last_update_check` é gravado em segundos Unix UTC no início da tentativa;
  falha também conta, para não transformar offline em loop de tentativas.
- Um pequeno jitter determinístico (0–6 h, derivado de caminho/PID/tempo) evita
  que todas as instalações consultem no mesmo minuto.
- A checagem manual ignora a janela de 24 h, mas tem debounce de 60 s.
- Guardar o `ETag` da última resposta e enviar `If-None-Match`; `304` significa
  “sem mudança”. **Importante:** a documentação do GitHub só isenta do rate
  limit condicional quando a requisição está autenticada; o Dish não usa token.
  Portanto o `304` economiza banda, mas continua contando no limite anônimo de
  60 req/h. A cadência diária é o controle real.
- Em `403`/`429`, respeitar `Retry-After`/`X-RateLimit-Reset`, não repetir
  automaticamente e mostrar erro apenas na checagem manual.
- Sem token, sem login, sem telemetria. O IP do usuário aparece para o GitHub
  como em qualquer download; `DISH_NO_UPDATE_CHECK=1` desliga a checagem
  automática.

### 4.8 Variáveis de ambiente

| Variável | Efeito |
| --- | --- |
| `DISH_NO_UPDATE_CHECK=1` | desliga a checagem automática; o botão manual continua disponível (uma ação explícita do usuário) |
| `DISH_UPDATE_URL` | usa um `latest.json` estático em vez da descoberta GitHub; assinatura em `<url>.minisig`; HTTPS, com HTTP permitido só para loopback de teste |
| `DISH_TEST_BIN` | já existe para os testes de UI; não interfere na atualização |

Não há variável para desligar a verificação de assinatura. Não há `--force`,
`--skip-signature` ou opção de instalar binário não assinado.

## 5. Modelo de confiança

### 5.1 Cadeia de autoridade

1. **Chave pública minisign embutida no binário.** Não é buscada na rede.
2. **Assinatura destacada** de `latest.json` (bytes exatos). Ela cobre todos os
   `sha256` dos assets.
3. **SHA-256 + tamanho** do `.tar.gz` baixado, conferidos contra o manifesto
   assinado.
4. **`BUILD.json`** dentro do arquivo: versão, target, glibc.
5. **`bin/dish --version`** do binário preparado, executado só depois de todas
   as verificações.
6. **`rename`** atômico para o destino, com backup do anterior.

O checksum sozinho não autentica nada: ele vem do mesmo GitHub que serve o
arquivo. A assinatura é o que separa “corrupção” de “substituição maliciosa”.

### 5.2 Ordem de verificação

1. TLS válido, host permitido, no máximo 5 redirecionamentos, todos HTTPS.
   Allowlist padrão: `api.github.com`, `github.com`,
   `release-assets.githubusercontent.com` e `objects.githubusercontent.com`.
   Em modo `DISH_UPDATE_URL`, o host de origem do manifesto e loopback também
   são aceitos para os assets de teste; o modo padrão nunca aceita outro host.
2. Baixar manifesto com limite de 64 KiB.
3. Verificar a assinatura **antes** de fazer `serde_json::from_slice`.
4. Validar schema, versão, tag, canal, target, glibc e assets.
5. Confirmar que a candidata é maior que a atual.
6. Baixar arquivo com limite de 128 MiB, calculando SHA-256 no stream; abortar
   se o tamanho estourar antes de terminar.
7. Comparar hash e tamanho com o manifesto; conferir `.sha256` do release como
   consistência adicional.
8. Extrair em diretório privado, recusando `..`, caminho absoluto e symlink que
   escape; exigir um único diretório de topo e `bin/dish` regular/executável.
9. Ler `BUILD.json` e comparar `version`, `target`, `minimum_glibc`.
10. Copiar o binário para um temporário no diretório de destino, `fsync`,
    executar `--version` e comparar a saída.
11. Backup + `rename` + `fsync` do diretório.

Qualquer falha em 1–10 mantém a versão atual e apaga temporários. Falha em 11 é
reportada e também mantém o destino atual (o backup só é removido depois do
sucesso).

### 5.3 Chave, assinatura e rotação

- Formato: minisign, chave pública embutida como constante no código; a chave
  privada nunca entra no repositório, no binário, no archive ou nos logs.
- A assinatura é **destacada** (`latest.json.minisig`), não um campo do JSON
  (um arquivo não pode assinar a si mesmo). O comentário confiável do minisign
  não é usado para decidir versão nem canal.
- Recomendação de custódia: chave privada num **environment protegido do
  GitHub Actions** (`release`), com revisor obrigatório, usado só pelo job de
  publicação do manifesto. Alternativa equivalente para o cliente: assinatura
  offline pelo mantenedor e upload manual de `latest.json` + `.minisig`; nesse
  caso o workflow gera o JSON como artifact e o checklist exige a assinatura
  antes de considerar o release pronto.
- A chave tem um `key id`; o cliente só aceita a assinatura se o id corresponder
  a uma chave pública embutida. Rotação: a versão nova embute a chave seguinte
  e é assinada pela chave atual; a antiga é removida depois de uma janela de
  migração.
- Chave comprometida é um incidente de segurança: publicar um release assinado
  pela chave alternativa, avisar no README/release e exigir atualização manual
  verificada. Não há caminho de “auto-recuperação” sem confiança.
- Testes usam um par de chaves **de teste**, nunca a chave real; fixtures de
  assinatura válida/inválida ficam em `tests/fixtures/update/`.

### 5.4 Ameaças e resposta

| Ameaça | Resposta |
| --- | --- |
| Corrupção no download | SHA-256/tamanho + falha fechada; binário atual mantido |
| Conta GitHub comprometida troca assets/manifesto | assinatura minisign falha; nenhum banner/download/aplicação |
| MITM / TLS inválido | cliente recusa; sem “continuar mesmo assim” |
| Asset de outro host/redirect inesperado | allowlist + limite de redirect; recusa |
| Manifesto com caminho absoluto/`..`/symlink | extração segura; recusa |
| Downgrade malicioso | comparação SemVer; candidata precisa ser maior |
| Arquivo incompatível com a glibc | `BUILD.json` + checagem local; não substitui |
| Substituir binário em uso | temporário + `rename`; nunca truncar; lock |
| Chave privada vazada | rotação/incidente; sem confiança automática nova |
| Usuário local hostil | atualização só no diretório do próprio usuário; sem root; assinatura |
| Anúncio falso de versão | assinatura do manifesto; canal/tag/versão consistentes |

### 5.5 O que não confiar

- No `digest` da API do GitHub como única prova (ele pode ser atualizado junto
  com o asset); o manifesto assinado é a autoridade.
- No `.sha256` sozinho: ele está no mesmo canal e não autentica o publicador.
- No `published_at` para decidir qual versão é mais nova.
- Na ordem de `created_at` da API para escolher a candidata.
- Em qualquer chave pública buscada na rede.
- Em URL de asset que não esteja na allowlist, mesmo que venha no manifesto.

## 6. UX

### 6.1 Banner global

Uma faixa fina, abaixo da barra de título e acima do conteúdo, no `Workspace` —
não dentro de uma conversa. Usa os tokens de `src/theme.rs` (hairline, superfície
e accent), versão em `FONT_MONO`, sem card e sem modal.

- Fase 1: `Dish 0.1.0-alpha.6 disponível` + `Notas` + `Baixar` + dispensar.
- `.deb`: `Baixar` abre a página do release; nenhuma instrução de `sudo` é
  executada.
- Fase 2 portátil: `Atualizar` inicia o download; durante o download mostra
  progresso; ao terminar, `Reiniciar agora` / `Depois`.
- `Esc` e o botão `×` dispensam; a dispensa é por versão
  (`dismissed_update_version`) e não volta para a mesma versão.
- Nunca bloqueia a abertura, nunca rouba foco, nunca aparece por cima de um
  modal. Se houver modal/confirmação, o banner espera.
- Erros automáticos são silenciosos; erros de uma ação manual viram toast ou
  texto na tela **Sobre / Pi**.

IDs de teste estáveis: `update-banner`, `update-notes`, `update-download`,
`update-apply`, `update-dismiss`, `settings-update-check`,
`settings-update-status`.

### 6.2 Configurações → Sobre / Pi

- Mostrar `Versão do Dish: 0.1.0-alpha.5` (ou `0.1.0 (build local)`), não só a
  versão-base.
- Botão **Verificar atualizações**; durante a checagem, spinner curto; ao
  terminar, “Você está na versão mais recente” ou a versão disponível com link.
- Seletor de canal **Pré-release / Estável**, com o padrão derivado da versão.
- Se `DISH_NO_UPDATE_CHECK=1`, explicar que a checagem automática está desligada
  e que o botão manual continua disponível.
- O botão não fecha a tela e não reinicia nada.

### 6.3 Textos e estados

Mensagens em português, curtas, sem prometer sucesso antes da verificação:

- `Verificando atualizações…`
- `Dish 0.1.0-alpha.6 disponível`
- `Você está na versão mais recente (0.1.0-alpha.5).`
- `Não foi possível verificar agora. Tente novamente.`
- `Atualização pronta. Feche e abra o Dish para usar a versão nova.`
- `Assinatura inválida; a versão atual foi mantida.` (somente em ação manual)

Nunca mostrar “atualizado com sucesso” antes de `rename` + `fsync`; nunca
mostrar detalhes de URL/credencial nos logs.

## 7. Preferências e estado

Novos campos serializados em `src/preferences.rs` (com `#[serde(default)]` no
struct, como hoje):

| Campo | Tipo | Uso |
| --- | --- | --- |
| `update_channel` | `Option<UpdateChannel>` | `None` deriva da versão; `Some` é escolha explícita |
| `last_update_check` | `Option<i64>` | segundos Unix UTC da última tentativa |
| `last_update_etag` | `Option<String>` | ETag da listagem/manifesto estático |
| `dismissed_update_version` | `Option<String>` | banner dispensado para essa versão |

O estado transitório da atualização (verificando, disponível, baixando,
verificando arquivo, pronto, erro) vive em memória no `Workspace`; não é
persistido. O download e o lock vivem no sistema de arquivos; não são campos de
preferência. Preferências continuam usando `preferences::save` e o único
escritor existente (`src/preferences.rs:88-109`), nunca escrita direta no
`state.json`.

Máquina de estados:

```text
Idle -> Checking -> UpToDate
                 -> Available -> Downloading -> Verifying -> Ready
                 -> SilentError (automático) / Message (manual)
Downloading -> Cancelled -> Idle
Verifying -> Failed -> Available (com erro)
Ready -> Applied (rename concluído) -> Idle ao fechar/reabrir
```

`Checking` e `Downloading` nunca bloqueiam a thread da UI: o trabalho roda no
`background_executor`/thread própria e o resultado volta por canal, como em
`src/rpc.rs`. Não há segundo escritor no stdin do Pi; atualização é um subsistema
separado.

## 8. Fatiamento em PRs

Cada PR deve ser independente, passar `cargo test --locked` e não misturar
mudança de comportamento com refatoração ampla.

| PR | Fase | Arquivos principais | Entrega |
| --- | --- | --- | --- |
| 1 | — | `docs/update-design.md` | este documento; nenhum código |
| 2 | 1 | `scripts/package-linux.py`, `src/main.rs`, `src/settings.rs`, `tests/test_release_package.py`, `docs/releasing.md`, `README.md` | embutir `DISH_RELEASE_VERSION`, `--version`, versão completa no Sobre, teste archive↔BUILD |
| 3 | 1 | `scripts/package-manifest.py` (novo), `tests/test_release_manifest.py` (novo), `.github/workflows/release.yml` (novo), `docs/releasing.md`, `docs/linux-release.md` | gerar/validar/assinar `latest.json`, publicar `.minisig`, documentar verificação |
| 4 | 1 | `Cargo.toml`, `Cargo.lock`, `licenses/overrides.json`, `src/update.rs` (+ `src/update/manifest.rs`, `src/update/http.rs`), `src/preferences.rs`, `src/workspace.rs`, `src/settings.rs`, `src/ui/update.rs`, `src/ui/mod.rs`, `tests/fixtures/update/`, `tests/update_smoke.py`, `docs/linux-release.md` | cliente de checagem, manifesto, banner, checagem manual, canal, testes |
| 5 | 2 | `Cargo.toml`, `Cargo.lock`, `licenses/overrides.json` (se necessário), `scripts/install-release.sh`, `scripts/uninstall-release.sh`, `src/update/install.rs`, `src/update.rs`, `src/workspace.rs`, `src/ui/update.rs`, `src/main.rs`, `src/fs_security.rs` (se necessário), `tests/test_release_package.py`, `tests/update_smoke.py`, `docs/linux-release.md` | aplicar no lugar, backup/rollback, lock, `dish.old`, `--update`, reinício seguro |
| 6 | 3 | `docs/update-design.md` (decisão), `docs/apt-vs-appimage.md` (novo, se aprovado) | decisão APT vs AppImage com dados; nenhum código antes da aprovação |

Dependências e cuidados:

- O PR 2 precisa vir antes do 3 e do 4: o manifesto e o cliente dependem do
  rótulo de versão correto.
- O PR 3 pode ser revisado com testes offline; o job de release só roda em
  `release: published`, com `contents: read` por padrão e `contents: write` só
  no job de upload, como o workflow do Windows já faz
  (`.github/workflows/windows.yml:109-149`).
- O PR 4 adiciona dependências novas; `licenses/overrides.json` e o bundle de
  licenças precisam ser revisados, e `scripts/package-linux.py` deve continuar
  falhando fechado.
- O PR 5 não move a escrita para `/usr/bin` nem usa `sudo`; `src/installation.rs`
  continua sendo o instalador do **Pi**, não o atualizador do Dish.
- `.github/workflows/ci.yml` não é alterado nesta frente. Quando a frente de CI
  permitir, ele ganha o teste do manifesto e o smoke de atualização; o job de
  publicação de release é um arquivo novo, para não acoplar o build de PR.
- O PR 5 deve coordenar com o PR 2 sobre a versão embutida: o binário do arquivo
  e o `BUILD.json` precisam concordar antes de qualquer self-update.

Em resumo, por fase:

- **Fase 1** toca `src/update*`, `src/preferences.rs`, `src/workspace.rs`,
  `src/settings.rs`, `src/ui/update.rs`, `src/ui/mod.rs`, `src/main.rs`,
  `scripts/package-linux.py`, `scripts/package-manifest.py` e testes/docs. Não
  toca `/usr/bin`, não usa `sudo`, não substitui binário.
- **Fase 2** toca `scripts/install-release.sh`, `scripts/uninstall-release.sh`,
  `src/update/install.rs`, os pontos de UI do banner/reinício, testes e
  `docs/linux-release.md`. Não toca `.github/workflows/ci.yml`, `/usr/bin` ou
  arquivos do Pi.
- **Fase 3** é só decisão/documentação; se aprovada, ganha uma frente própria
  para APT ou AppImage, sem reutilizar o caminho de self-update portátil para
  escrever em `/usr`.

## 9. Testes

### Unitários (Rust)

- Comparação SemVer: os casos da Seção 4.5, incluindo `alpha.9 < alpha.10`,
  `alpha < alpha.1`, pré-release < final e build metadata ignorada.
- Seleção de canal e candidata; nunca downgrade; igualdade não atualiza.
- Parse do manifesto: schema, campos obrigatórios, `tag`/`version`, assets
  duplicados, URL/host inválido, checksum/tamanho inválidos, `minimum_glibc`.
- Forma de instalação: `$HOME/.local/bin/dish` regular, symlink, `/usr/bin`,
  caminho extraído, `$HOME` ausente/relativo, diretório sem permissão.
- Cadência: 24 h, jitter, `last_update_check` no futuro, `304`, rate limit.
- Assinatura: fixture válida, byte alterado, chave errada, assinatura ausente.
- Estado de preferências: campos novos ausentes em JSON antigo; round-trip.

### Integração sem rede

- Servidor HTTP local em `127.0.0.1` servindo `latest.json`, `.minisig` e um
  arquivo falso; `DISH_UPDATE_URL` aponta para ele, `HOME`/`XDG_*` temporários.
- Casos: versão nova, versão igual, versão menor, assinatura inválida, checksum
  errado, tamanho errado, `BUILD.json` divergente, glibc insuficiente, arquivo
  malformado, 404, timeout, 403/429.
- Fase 2: diretório read-only, destino symlink, lock ocupado, binário em
  execução, falha de `rename`, rollback com `dish.old`, `uninstall.sh` remove
  temporários e backup sem tocar em dados do usuário.
- Nenhum teste usa a chave real, sudo, rede externa ou `$HOME` real.

### UI

- `tests/update_smoke.py` com Xvfb e Pi sintético:
  - manifesto local com versão nova -> banner aparece com o texto e os IDs;
  - dispensar -> banner some; reabrir a mesma versão -> não volta; versão maior
    -> volta;
  - “Notas” e “Baixar” abrem a URL esperada (interceptar/observar, sem rede);
  - **Verificar atualizações** em Sobre mostra o resultado;
  - assinatura/manifesto inválidos -> nenhum banner e nenhum crash;
  - `DISH_NO_UPDATE_CHECK=1` -> nenhuma checagem automática.
- Fase 2: progresso/erro/ready; “Reiniciar agora” desabilitado com execução em
  andamento ou rascunho; nunca reiniciar sozinho.
- Preferir texto/ID a coordenadas, como os outros smokes.

### Release

- `scripts/package-manifest.py` gera o JSON; teste verifica que cada
  `name`/`size`/`sha256` corresponde ao asset real e ao `.sha256`.
- `latest.json` assinado verifica com a chave pública embutida (fixture de
  teste); manifesto alterado falha.
- `BUILD.json.version` do arquivo == `bin/dish --version`.
- O job de release falha se faltar asset, se o `.sha256` não bater, se o
  `tag`/versão divergir ou, quando a chave está no CI, se a assinatura não for
  anexada. Com assinatura offline, o checklist de publicação faz essa
  verificação. Nunca usa `--clobber`.

Comandos de referência (executar antes de publicar):

```sh
TMPDIR=/tmp cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/update_smoke.py
```

## 10. O que não entra

- Atualizar o **Pi**. O Dish é cliente; o instalador do Pi continua em
  `src/installation.rs` e tem o próprio canal.
- Windows e macOS. O instalador NSIS existe, mas não é alvo deste desenho; um
  `--version` do Windows continua usando o fallback local até uma frente
  própria.
- Delta updates, bsdiff, downloads parciais ou retomada.
- Servidor próprio de atualização, CDN própria ou conta de serviço.
- Atualização automática de `.deb` com `pkexec`, `sudo`, `apt` ou escrita em
  `/usr`.
- Atualização automática de build local, archive extraído sem instalar, symlink
  ou instalação desconhecida.
- Downgrade automático, “force update” ou instalação sem assinatura.
- Reinício automático, fechamento de conversa em execução ou envio de prompt
  durante o relançamento.
- Telemetria, identificador de instalação, analytics de versão ou relato
  automático de falhas.
- Assinatura de artefatos do Pi ou de terceiros.

## 11. Riscos e mitigação

| Risco | Impacto | Mitigação |
| --- | --- | --- |
| Chave de assinatura comprometida | crítico | chave só em environment protegido/offline, revisão obrigatória, rotação, incidente documentado |
| Conta GitHub comprometida | alto | assinatura do manifesto; assets sem `--clobber`; clientes recusam manifesto não assinado |
| Dependências novas (HTTP/TLS/tar) e licenças | médio | PR dedicado, licenças no bundle, `licenses/overrides.json` hash-pinned, `--locked`, revisão do diff |
| `install.sh --update` depende de bash/python3 | baixo | falha fechada antes do `rename`; manter a versão atual e documentar a dependência já existente no instalador |
| API do GitHub muda ou limita | médio | versão de API fixada, ETag, cadência diária, erro silencioso, `DISH_UPDATE_URL` para teste/mirror |
| Release parcial (asset/manifesto faltando) | médio | gerador só depois dos assets; falha fechada; não cair para release anterior; checklist de publicação |
| glibc incompatível | alto em distros antigas | `minimum_glibc` no manifesto/BUILD; recusa antes de substituir; manter versão atual |
| Substituição durante execução | médio | temp + `rename`, backup, lock, sem truncar; `ETXTBSY` só para spawn |
| Reinício perde trabalho | alto | sem reinício automático; bloquear com execução/rascunho; restaurar última sessão salva |
| Notificação excessiva em alpha | médio | 1×/dia, dispensa por versão, sem modal |
| Privacidade (IP para GitHub) | baixo | sem token/telemetria, opt-out, log sem URL/credencial |
| Versão-base vs rótulo de release | alto | `DISH_RELEASE_VERSION` obrigatório no pacote; teste archive↔BUILD; build local marcado |
| `.deb` antigo em `/usr/bin/dish` | médio | detectar e orientar migração; nunca sobrescrever |

## 12. Decisões que dependem do dono

1. **Assinar o manifesto já na Fase 1?** Recomendação: sim. Sem isso, a Fase 1
   é apenas uma dica não autenticada e a Fase 2 exigiria uma migração de
   confiança. Se o dono recusar, o cliente deve continuar sem banner em
   manifesto não assinado (não aceitar “checksum apenas”).
2. **Custódia da chave minisign:** environment protegido do GitHub Actions com
   revisor obrigatório (recomendado, publicação automatizada) ou assinatura
   offline com upload manual (mais forte contra comprometimento do CI, mais
   operação). O cliente é o mesmo nos dois casos.
3. **Orçamento de dependências:** aprovar `ureq` (ou `minreq`) com rustls,
   `minisign-verify`, `semver`, `sha2`, `tar` e `flate2`, com as entradas de
   licença correspondentes. Recomendação: `ureq` + rustls; a PR mede tamanho,
   build e licenças e pode trocar o cliente HTTP sem mudar o contrato.
4. **Canal padrão:** derivar da versão em execução (pré-release continua
   pré-release; estável começa estável) e permitir escolha em Sobre. O dono
   confirma se quer outro padrão.
5. **Reinício:** recomendação é nunca automático e bloquear com execução ou
   rascunho; “Reiniciar agora” é explícito. O dono pode preferir apenas
   “fechar e reabrir”.
6. **Fase 3:** escolher APT ou AppImage depois dos dados de download/uso; não
   iniciar infraestrutura APT durante alpha sem essa decisão.

## 13. Perguntas da issue resolvidas

- **minisign ou sigstore/cosign?** minisign nas Fases 1–2: uma chave, uma
  assinatura destacada, cliente pequeno e auditável. cosign/sigstore pode ser
  hardening futuro, mas não é pré-requisito.
- **AppImage vale um quarto asset?** Ainda não; é candidato da Fase 3, junto com
  APT. Não adicionar formato sem decisão de suporte.
- **`install.sh --update`?** Sim na Fase 2, para o caminho manual e o automático
  compartilharem a mesma colocação de arquivos e a troca atômica.
- **Repositório APT durante alpha?** Não. Primeiro medir a base `.deb`; o custo
  de chave/repositório não se paga enquanto tudo é pré-release e glibc-gated.

## 14. Evidências verificadas

| Afirmação | Onde foi verificado |
| --- | --- |
| Versão-base `0.1.0` | `Cargo.toml:3` |
| Sobre usa `CARGO_PKG_VERSION` | `src/settings.rs:1226` |
| `--version` é encaminhado ao Pi | `src/main.rs:188-211` |
| Sem cliente HTTP; `curl` só para o Pi | `Cargo.toml:8-21`, `src/installation.rs:231`, `396-398` |
| Portátil instala em `~/.local/bin/dish` | `scripts/install-release.sh:34-44` |
| `.deb` atual instala `/usr/bin/dish-pi` | `scripts/package-deb.py:30`, `178-184` |
| `.deb` antigo usava `/usr/bin/dish` | `docs/linux-release.md:45-54` |
| `BUILD.json` e campos | `scripts/package-linux.py:113-115` |
| `BUILD.json` não é instalado pelo `.deb` | `scripts/package-deb.py:178-200` |
| Retry `ETXTBSY` cobre spawn | `src/installation.rs:293-326` |
| Preferências usam escrita atômica de um escritor | `src/preferences.rs:88-109`, `src/fs_security.rs:40-73` |
| Sessão/pasta restauradas sem argumentos | `src/workspace.rs:2124-2141`, `src/main.rs:192`, `226-233` |
| `is_busy` disponível para bloquear reinício | `src/state.rs:2773-2775` |
| CI Linux é somente leitura e não publica | `.github/workflows/ci.yml:15-16`, `72-145` |
| Windows publica no evento `release` | `.github/workflows/windows.yml:109-149` |
| Todos os releases atuais são prerelease | API `GET /repos/cleisonsantos/dish/releases` em 2026-10-25 |
| `/releases/latest` retorna 404 hoje | API `GET .../releases/latest` em 2026-10-25 |
| Assets e checksums do alpha.5 | `gh release view v0.1.0-alpha.5` e download verificado por `sha256sum -c` |
| Redirect de asset para `release-assets.githubusercontent.com` | resposta 302 observada em 2026-10-25 |
| Licenças dos crates candidatos | crates.io: `minisign-verify` MIT; `ureq`, `semver`, `sha2`, `tar`, `flate2` MIT OR Apache-2.0 |

Referências externas usadas para o contrato do GitHub:

- [REST API — releases](https://docs.github.com/en/rest/releases/releases)
- [Boas práticas — requisições condicionais](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [Rate limits — não autenticado, 60 req/h](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
- [SemVer 2.0.0](https://semver.org/lang/pt-BR/)
- [minisign](https://jedisct1.github.io/minisign/)

Itens marcados como “a confirmar” na implementação: versão exata do header
`X-GitHub-Api-Version` vigente no momento do PR; compatibilidade do
`minisign-verify` com a versão do CLI que assinar; trust store TLS do cliente
escolhido; e os números que basearão a decisão da Fase 3.
