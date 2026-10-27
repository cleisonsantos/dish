# Provedores e autenticação (#69) — contrato, reconhecimento e limites

O Dish não autentica nada. O Pi é dono do OAuth, do refresh e do armazenamento;
o Dish lê estado e mostra, e quando precisa escrever, escreve **pelo Pi**. Este
documento fixa essa fronteira, o protocolo do adaptador e o que foi medido
contra o Pi real, para a interface não depender de suposições.

As fases 1–3 da #69 já estão mescladas. A fase 3b acrescentou a releitura das
conversas abertas; o PR 1 da fase seguinte corrige a promessa dessa releitura,
porque o Pi calcula o catálogo de modelos no startup. O contrato do adaptador
foi medido originalmente contra o **Pi 1.2.0**; o reconhecimento abaixo foi
refeito contra o **Pi 1.4.0** desta máquina.

- Pi medido: `@earendil-works/pi-coding-agent` **1.4.0**, instalação npm global
  em `~/.hermes/node/lib/node_modules/@earendil-works/pi-coding-agent`. Os
  caminhos `dist/...` são relativos a esse diretório.
- SDK embutido: `@earendil-works/pi-ai` **1.4.0**.
- Node: `v22.22.3`. Dish de referência do reconhecimento: `b88188e`
  (`origin/main`) na branch `feat/69-providers`.
- Nenhum valor de credencial aparece aqui, nem parcialmente: onde era preciso
  provar que um campo existe, está escrito o **nome do campo** e o **tamanho
  aproximado**.

## O problema

Um usuário novo pode terminar a instalação do Pi e do Dish sem nenhum modelo
disponível, e sem um caminho pela interface para entender o motivo. Medido:
`get_available_models` no RPC devolve **0 modelos** quando não há credencial —
o RPC não expõe o catálogo de provedores, só o que já está utilizável. Também
não existe `login`/`logout` no RPC, e mandar `/login` como prompt está fora de
questão (a issue é explícita: não enviar segredo nem comando de TUI ao agente).

## Onde o Pi guarda credenciais

### Mapa de arquivos

| Caminho | Papel | Evidência |
| --- | --- | --- |
| `~/.pi/agent/` | diretório do agente; `PI_CODING_AGENT_DIR` troca a raiz | `dist/config.js:476`, `:491-497` |
| `~/.pi/agent/auth.json` | **único** armazenamento de credenciais de provedor (chave de API e OAuth) | `dist/config.js:507-509`, `dist/core/auth-storage.js:31` |
| `~/.pi/agent/models.json` | configuração **escrita pelo usuário**: provedores, `apiKey`, `baseUrl`, modelos | `dist/config.js:503-505`, `dist/core/model-config.js:125-157` |
| `~/.pi/agent/models-store.json` | cache de catálogos remotos; **não** é credencial e **não** é catálogo completo | `dist/core/models-store.js:30`; medido: só os provedores configurados |
| `~/.pi/agent/settings.json` | preferências (`defaultProvider`, `defaultModel`…). Medido: nenhuma chave de credencial | medição, chaves de topo apenas |
| `~/.pi/agent/mcp-auth.json` | tokens OAuth de servidores **MCP** (fora do escopo de provedores) | `dist/extensions/mcp/oauth.js:88` |

Não há cofre do sistema: nenhuma referência a `keytar`, `libsecret` ou
`secret-service` no pacote. No Windows o mesmo arquivo é usado; `mode: 0o600` só
tem efeito limitado pelas ACLs do sistema.

### Formato de `auth.json`

Um objeto `providerId → credencial`, com dois tipos
(`dist/core/auth-storage.js:189-209`):

```jsonc
{ "<provider>": { "type": "api_key", "key": "…", "env": { "NOME": "…" } },
  "<provider>": { "type": "oauth", "access": "…", "refresh": "…",
                  "expires": <epoch ms>, "accountId": "…" } }
```

Medido nesta máquina (inspeção com Python, só nomes de campo e tamanhos):
dois provedores configurados, um `api_key` (campos `type` + `key`, chave com
~50 caracteres) e um `oauth` (campos `type` + `access` + `refresh` + `expires` +
`accountId`). Modo do arquivo `600`, diretório `700`; o Pi cria com `mode: 0o600`
(`dist/core/auth-storage.js:27`) e o diretório com `0o700` (`:34-38`).

O campo opcional `env` de uma credencial `api_key` **vence o ambiente do
processo** para aquele provedor (`docs/providers.md:90-101` do pacote do Pi).

### Escrita, concorrência e migração

- Toda escrita passa por `proper-lockfile` sobre o próprio `auth.json`: lock
  síncrono com 10 tentativas (`:45-60`), lock assíncrono com `stale` de 30s,
  retry com backoff e retentativa de `EPERM` no Windows (`:81-140`).
- `modify()` faz leitura-modificação-escrita sob o lock e **preserva os outros
  provedores** (`:385-403`); `delete()` idem (`:403-412`), e `list()` devolve só
  `{providerId, type}` (`:414-420`).
- Leituras revalidam pelo *revision* do arquivo (`:337-374`), então um processo
  do Pi já rodando enxerga uma credencial escrita por outro processo na próxima
  leitura.
- `oauth.json` e `apiKeys` do `settings.json` legado são migrados para
  `auth.json` (`dist/migrations.js:13-63`).
- **Efeito colateral medido:** instanciar `AuthStorage`/`ModelRuntime` num
  diretório de agente sem `auth.json` **cria** `auth.json` com `{}` (2 bytes,
  `600`). O SDK já traz `ReadOnlyAuthStorage` (`:164-240`), usado pelo próprio
  Pi em `pi auth check --no-refresh` (`dist/main.js:139`), e o probe do Dish
  ainda não o usa.

## O que o Pi oferece hoje

Medido com `pi --version` = `1.4.0`.

| Comando | Existe? | O que faz | Evidência |
| --- | --- | --- | --- |
| `pi auth check --provider P [--model M] [--json] [--no-refresh]` | sim | responde `{status, provider, reason}` ou `authType`; `0` = `ready`, `1` = `not_ready`, `2` = `invalid` | `dist/cli/auth-command.js:3-21`, `dist/main.js:162` |
| `pi auth print-api-key` | sim | imprime o segredo em texto puro | `dist/cli/credential-print.js` |
| `pi auth print-bearer-token` | sim | idem, refresh de OAuth antes | idem |
| `pi login` / `pi logout` | **não** | sem dispatch; `pi login --help` imprime o help global | `dist/package-manager-cli.js:330-341`, `dist/cli/auth-command.js:32-44` |
| `pi models` / `pi providers` | **não** | idem | idem |
| `pi --list-models` | sim | lista modelos **dos provedores já utilizáveis** | `dist/cli/list-models.js:34-41` |
| `pi --api-key <chave>` | sim | credencial em memória para o processo (`source = runtime`) | `pi --help` |
| `/login`, `/logout` (TUI) | sim | único caminho de login/logout no Pi | `dist/modes/interactive/interactive-mode.js:2595`, `:4947`, `:4980` |
| `pi mcp login|logout` | sim | OAuth de servidor MCP, grava `mcp-auth.json` — não é provedor | `pi --help`, `dist/extensions/mcp/oauth.js:88` |

O RPC **não** expõe autenticação: os comandos são `prompt`, `steer`,
`follow_up`, `abort`, `clear_queue`, `new_session`, `get_state`, `get_messages`,
`set_model`, `cycle_model`, `get_available_models`, thinking, queue modes,
compaction, retry, bash, sessão e `get_commands`
(`dist/modes/rpc/rpc-mode.js:298-540`). Não há `login`, `logout`,
`list_providers` nem `refresh_models`.

Saídas medidas, sem segredo algum:

```
$ pi auth check --provider deepseek --json --no-refresh
{"status":"not_ready","provider":"deepseek","reason":"credentials_not_configured"}   [exit=1]

$ pi auth check --provider openai-codex --json --no-refresh
{"status":"ready","provider":"openai-codex","authType":"oauth"}                      [exit=0]

$ pi auth check --provider opencode-go --json --no-refresh
{"status":"ready","provider":"opencode-go","authType":"api_key"}                     [exit=0]

$ pi --list-models | wc -l
37        # 36 linhas de modelo, apenas 2 provedores (openai-codex, opencode-go)
```

**`ready` não significa "chave validada no serviço".** Com um `auth.json`
isolado e uma chave falsa que nunca foi usada, `pi auth check --json` responde
`{"status":"ready","provider":"deepseek","authType":"api_key"}` (exit 0):
`checkAuth()` só resolve a credencial localmente
(`dist/core/model-runtime.js:310-312` → `pi-ai/dist/models.js:288-296`). O mesmo
vale para `getProviderAuthStatus`, que só responde "existe e de onde vem"
(`dist/core/model-runtime.js:441-451`).

## Precedência de credenciais

Ordem em `ModelRuntime.getProviderAuthStatus()`
(`dist/core/model-runtime.js:441-451`):

1. `runtime` — chave em memória (`setRuntimeApiKey`, `--api-key`);
2. `stored` — `auth.json`;
3. `models.json` — `apiKey` do provedor: `models_json_command` para `!comando`,
   `environment` (com o nome da variável) para `"$VAR"`, `models_json_key` para
   literal, `fallback` para configuração de extensão
   (`dist/core/provider-composer.js:478-491`);
4. `environment` — variável de ambiente do provedor; o `label` com o nome da
   variável vem do `resolve` de cada provedor
   (`pi-ai/dist/auth/helpers.js:15-30`);
5. `configured: false`.

O `resolve()` de cada provedor confirma a mesma ordem no momento da requisição:
credencial salva vence, depois as variáveis na ordem declarada
(`pi-ai/dist/auth/helpers.js:15-30`). Provedores com resolução especial repetem
a regra; `anthropic` é o caso mais longo: salva → `ANTHROPIC_AUTH_TOKEN` →
`ANTHROPIC_OAUTH_TOKEN` → `ANTHROPIC_API_KEY` → workload identity federation
(`pi-ai/dist/providers/anthropic.js:15-57`).

Matriz medida (diretório de agente isolado, valores falsos, sem rede):

| `auth.json` | ambiente | `models.json` | `source` reportado |
| --- | --- | --- | --- |
| — | `DEEPSEEK_API_KEY` | — | `environment`, label `DEEPSEEK_API_KEY` |
| `deepseek` `api_key` | `DEEPSEEK_API_KEY` | — | `stored` |
| — | — | `apiKey` literal | `models_json_key` |
| — | `DS_KEY_X` | `apiKey: "$DS_KEY_X"` | `environment`, label **`DS_KEY_X`** |
| — | — | `apiKey: "$DS_KEY_MISSING"` | não configurado |
| — | — | `apiKey: "!printf …"` | `models_json_command` |
| `deepseek` `api_key` | — | `apiKey` literal | `stored` |
| — | `DEEPSEEK_API_KEY` **e** `DS_KEY_X` | `apiKey: "$DS_KEY_X"` | `environment`, label `DS_KEY_X` |
| `auth.json` inválido | — | — | não configurado (o probe não quebra) |

Duas leituras importantes para a UI:

- `models.json` vence a variável de ambiente **do provedor**. Se a UI disser só
  "variável de ambiente", ela esconde que a origem real é o `models.json`; o
  `label` (`DS_KEY_X` vs `DEEPSEEK_API_KEY`) é o que distingue.
- `auth.json` vence tudo, menos a chave de runtime.

## De onde vem a lista de modelos

| Fonte | Conteúdo | Evidência |
| --- | --- | --- |
| Catálogo embutido no SDK | 42 provedores e **1599 modelos** nesta versão, mesmo sem credencial | medido com `allowModelNetwork:false`; `pi-ai/dist/providers/*.models.js`, `models.generated.js` |
| Catálogo remoto por provedor | atualização do catálogo embutido, com etag | `dist/core/model-runtime.js:83-96` |
| `models.json` | provedores e modelos do usuário; provedor novo exige `baseUrl` + `api` | `docs/models-configuration.md:9-33` |
| `models-store.json` | cache dos catálogos remotos (`models[]`, `checkedAt`, `lastModified`, `etag`), por provedor | `dist/core/models-store.js:30`; medido: só os 2 provedores configurados |
| Filtro por chave | OpenRouter lista apenas os modelos que a chave permite | `CHANGELOG.md:107` (1.3.0); `pi-ai/dist/providers/openrouter.js:66` |
| RPC `get_available_models` | modelos **dos provedores autenticados**, de um snapshot em memória | `dist/modes/rpc/rpc-mode.js:381-384` |

`models-store.json` **não** serve como catálogo e não contém segredo: ids,
nomes, api e etag.

Medições do catálogo sem nenhuma credencial nova:

```
$ node scripts/pi-auth-adapter.mjs --status        # (Dish, somente estado)
providers=42  with_api_key_login=41  with_oauth=9  total_models=1599
configured=[('openai-codex','stored'), ('opencode-go','stored')]
top5_models=[(401,'openrouter'), (305,'vercel-ai-gateway'), (193,'amazon-bedrock'), …]
```

## O que o Dish já faz, em uma página

| Peça | Arquivo | Observação |
| --- | --- | --- |
| Adaptador embutido | `src/providers.rs`, `scripts/pi-auth-adapter.mjs` | `include_str!` + cache privado; prazo de 20s |
| Node e localização do Pi | `src/providers.rs` | `--pi-bin`/`--package` a partir de `installation::pi_location` |
| Estado e origem | `src/providers.rs` | `Source` cobre `stored/runtime/environment/fallback/models_json_key/models_json_command` + `Unknown` |
| Login/remoção | `src/provider_login.rs` | JSONL, uma sessão por login, `Drop` mata a árvore de processos |
| Resposta secreta | `src/settings.rs` | valor vai para o adaptador e o editor é limpo em seguida |
| Remoção | `src/settings.rs` | confirmação + aviso "Remover aqui não revoga a chave no serviço" |
| Conversa aberta | `src/settings.rs`, `src/workspace.rs`, `src/state.rs` | credencial mudou → cada conversa relê o catálogo e compara com o adaptador |
| Reabrir conversa | `src/workspace.rs`, `src/state.rs`, `src/ui/mod.rs` | reinicia só aquele processo com `--session`, preservando rascunho e histórico |
| Contrato e testes | `docs/provider-auth.md`, `tests/test_provider_adapter.py`, `tests/test_provider_login.py`, `tests/providers_smoke.py` | os testes plantam segredos falsos e falham se algum aparecer |

## O que a UI pode fazer com segurança hoje

Cada item com a evidência que o sustenta.

1. **Listar todos os provedores, inclusive sem credencial** — 42 medidos;
   `ModelRuntime.getProviders` (`dist/core/model-runtime.js:283-291`) não filtra
   por autenticação, ao contrário do RPC (`rpc-mode.js:381-384`).
2. **Dizer a origem sem o valor** — `getProviderAuthStatus` devolve
   `{configured, source, label}` e nunca o segredo (`model-runtime.js:441-451`).
3. **Listar quais credenciais estão salvas, por tipo** —
   `listCredentials` devolve `{providerId, type}` (`model-runtime.js:438-439`).
4. **Conectar por chave de API** — o provedor declara `prompt({type:"secret"})`
   (`pi-ai/dist/auth/helpers.js:9-15`); o adaptador repassa a pergunta e o valor
   só viaja Dish → Pi.
5. **Conectar por conta do provedor (OAuth)** — 9 provedores com OAuth medidos
   (`anthropic`, `github-copilot`, `kimi-coding`, `meta`, `openai`,
   `openai-codex`, `openrouter`, `radius`, `xai`). O Pi emite `auth_url`,
   `device_code`, `progress` (`pi-ai/dist/auth/types.d.ts:130-145`,
   mapeado em `dist/core/provider-composer.js:196-215`). O fluxo varia por
   provedor: navegador + código colado (`auth_url`/`manual_code`) ou código de
   dispositivo (`device_code`, p.ex. xAI e Kimi Code).
6. **Cancelar** — `AbortController` do adaptador e `terminate_tree` no `Drop`
   (`src/provider_login.rs`).
7. **Remover a credencial salva, com confirmação e sem prometer revogação** —
   `runtime.logout` (`model-runtime.js:583-590`) e o texto de `src/settings.rs`.
8. **Reler o estado depois de cada ação, sem UI otimista** —
   `refresh_providers` após `Done`/`logout` (`src/settings.rs`).
9. **Não escrever `auth.json` à mão** — toda escrita do Pi é sob lock e preserva
   os outros provedores (`auth-storage.js:385-403`).
10. **Ser honesto quando não há como falar com o Pi** — sem Node ou sem pacote, a
    seção mostra o motivo e oferece o terminal (`src/settings.rs`,
    `src/providers.rs`).

## Protocolo entre Dish e adaptador

`scripts/pi-auth-adapter.mjs`, JSONL em stdin/stdout. Um `--status` responde e
sai; com `login`, o processo fica aberto até o fim da operação, porque o login
faz perguntas e espera respostas. Todos os erros são códigos fechados
(`package_not_found`, `package_invalid`, `probe_failed`, `bad_json`,
`bad_request`, `busy`, `unsupported_op`, `unsupported_type`, `login_failed`,
`logout_failed`, `cancelled`, `unknown_prompt`), nunca texto de exceção cru com
caminho de credencial.

```jsonc
// entrada
{"id": 1, "op": "status"}
{"id": 1, "op": "login", "provider": "anthropic", "type": "api_key"}
{"id": 2, "op": "reply", "prompt": 7, "value": "…"}   // resposta a uma pergunta
{"id": 3, "op": "cancel", "target": 1}                  // aborta o login 1
{"id": 4, "op": "logout", "provider": "anthropic"}

// saída
{"id": 1, "ok": true, "data": {
  "pi": {"version": "1.4.0", "package": "…/@earendil-works/pi-coding-agent"},
  "providers": [{"id": "anthropic", "models": 17, "configured": false,
                 "source": null, "label": null,
                 "auth": {"api_key": {"name": "Anthropic API key"}, "oauth": null}}],
  "credentials": [{"providerId": "openai", "type": "oauth"}],
  "readOnly": false
}}
{"id": 1, "prompt": 7, "ask": "secret", "message": "Enter Anthropic API key", "placeholder": null,
 "options": []}
{"id": 1, "notify": "auth_url", "url": "https://…", "instructions": "…"}
{"id": 1, "notify": "device_code", "userCode": "ABCD-1234",
 "verificationUri": "https://…", "intervalSeconds": 5, "expiresInSeconds": 900}
{"id": 1, "ok": true, "data": {"provider": "anthropic", "type": "api_key"}}
```

`ask` pode ser `text`, `secret`, `select` (com `options`) ou `manual_code`;
`notify` pode ser `info` (com `links`), `auth_url`, `device_code` ou `progress`.
O desfecho de um login informa **provedor e tipo** — nunca a credencial, que o
Pi já gravou. Um login por vez: uma segunda tentativa responde `busy`.

Localização do pacote do Pi, em ordem: `--package`, `DISH_PI_PACKAGE`, o prefixo
privado do Dish (`$XDG_DATA_HOME/dish/pi` ou `%LOCALAPPDATA%\dish\pi`), as
releases do instalador oficial (`~/.pi/agent/install/releases/*/node_modules/…`),
o npm global (`%APPDATA%\npm\node_modules`, `npm root -g`) e, por último, subir
a partir do executável do Pi (`--pi-bin`/`DISH_PI_BIN`), seguindo links
simbólicos. Um caminho explícito é **autoridade**: se não contém o pacote, o
adaptador falha em vez de usar outra instalação em silêncio.

## Contrato de segurança

O Dish nunca:

- chama `pi auth print-api-key`, `pi auth print-bearer-token` nem
  `pi auth check --credentials`;
- chama `runtime.getAuth()`, `login()` ou `logout()` fora do adaptador;
- guarda, registra ou exibe valor de credencial — nem em preferências, nem em
  log, nem em mensagem de erro, nem em relatório de falha;
- envia comando de autenticação como prompt para o agente;
- escreve em `auth.json`, `models.json` ou nas configurações do Pi.

O adaptador copia campo por campo o que o contrato permite (`id`, `models`,
`configured`, `source`, `label`, `providerId`, `type`), nunca espalha um objeto
recebido do SDK, e ainda passa cada string por uma rede de segurança que
substitui por `<redacted>` o que pareça chave (`sk-…`, `eyJ…`, `gh*_…`,
`Bearer …`). Respostas de `prompt` de login viajam do adaptador para o Pi e
morrem ali: não voltam para o Dish, não entram em histórico e são descartadas ao
cancelar.

## O que não dá

1. **Login/logout pela CLI do Pi.** Não existe `pi auth login|logout`; `/login` e
   `/logout` são só da TUI (`interactive-mode.js:2595`, `:4980`) e comando de
   TUI não executa via RPC: o `prompt` só reconhece comandos de extensão
   (`dist/core/agent-session.js:1636-1645`) — `/login` viraria texto. Por isso o
   adaptador precisa de Node e do pacote, e o Dish mostra "não verificável"
   quando um Pi autocontido não expõe o SDK.
2. **Validar a chave no serviço.** `ready`/`configured` significam "credencial
   resolvida localmente" — medido com chave falsa. Não há estado "autenticação
   com erro" por provedor; um erro só aparece quando uma requisição falha,
   dentro da conversa.
3. **Atualizar os modelos de uma conversa já aberta.** Medido: o processo do Pi
   calcula o snapshot no *startup* e `get_available_models` só devolve esse
   snapshot (`rpc-mode.js:381-384`; nenhum comando de refresh em `:298-540`).
   Sequência medida com `pi --mode rpc`, diretório isolado, chaves falsas:

   ```
   1. before:                     ['deepseek']
   2. after external login:       ['deepseek']          # credencial gravada por outro processo
   3. set_model anthropic/…:      "Model not found: anthropic/claude-haiku-4-5"
   4. after set_model attempt:    ['deepseek']
   ```

   Na direção inversa (credencial removida): a lista continua mostrando o
   provedor e `set_model` falha com `No API key for anthropic/claude-haiku-4-5`.
   Ou seja, `refresh_catalog` relê o mesmo snapshot.

   O PR 1 não finge que conserta isso. Depois de uma mudança de credencial, o
   Dish compara os modelos reais da conversa com o catálogo do adaptador:
   provedor configurado que falta nos modelos, ou provedor que continua nos
   modelos sem credencial, vira um aviso honesto com a ação **"Reabrir esta
   conversa"**. A ação só aparece habilitada sem streaming, sem fila e sem
   diálogo pendente; nesse caso o motivo aparece ao lado do botão.

   **O conserto definitivo é do lado do Pi**: um comando RPC de reavaliação do
   catálogo (ou o Pi revalidar o snapshot quando o `auth.json` mudar). Quando ele
   existir, "Reabrir esta conversa" vira uma chamada, com detecção de capacidade.
4. **`pi --list-models` como plano B.** Também filtra por provedor autenticado
   (`dist/cli/list-models.js:34-41`): medido, 36 modelos de 2 provedores.
5. **Provedores locais/compatíveis (`models.json`).** Não há fluxo na UI; a seção
   só aponta o arquivo. É a fase 4, deliberadamente separada.
6. **Saber o nome da variável de ambiente na tela.** O adaptador envia `label`,
   mas `Snapshot`/`Provider` não guardam esse campo: a linha mostra só "variável
   de ambiente". Fica para o PR 3.
7. **Exigir versão mínima do Pi.** O adaptador informa `pi.version` e a UI só
   exibe ("Catálogo do Pi 1.4.0"). Pi antigo sem os métodos esperados vira
   `probe_failed` genérico, não "versão não suportada".
8. **Mascarar o campo da chave.** O `Editor` do Dish não tem modo senha; o
   contrato registra isso como limite. Mascarar é uma mudança própria no editor,
   com testes de seleção e colagem, e fica para um PR separado.
9. **`PRODUCT.md` ainda diz o contrário.** `PRODUCT.md:28`: "Pi manages provider
   authentication in its terminal interface. Dish uses existing Pi credentials."
   O `README.md` já está atualizado.

## Riscos e efeitos colaterais medidos

| Risco | Medição | Consequência |
| --- | --- | --- |
| Probe cria arquivo | `--status --agent-dir <vazio>` deixou `auth.json` de 2 bytes (`600`) | inofensivo, mas contradiz "somente leitura"; `ReadOnlyAuthStorage` resolve no PR 3 |
| `--agent-dir` isola só credencial | o adaptador passa apenas `authPath`; `modelsPath` continua vindo de `getAgentDir()` (`model-runtime.js:81`) | um `models.json` real pode influenciar um teste isolado |
| Contagem de modelos sem personalização | `getModels().length` é o catálogo; OpenRouter filtra por chave em tempo de requisição | a contagem exibida pode ser maior que a lista utilizável |
| Dois escritores | lock do Pi é por arquivo e preserva outros provedores (`auth-storage.js:385-403`) | Dish não deve escrever `auth.json`; só o Pi |
| Renomeação de provedor | `azure-openai-responses` → `azure` em 1.0.4 | chaves antigas em `auth.json` ficam órfãs; a UI mostra o provedor sem credencial, sem inventar migração |
| Pi como binário autocontido | o adaptador procura `install/releases/*/node_modules/...`, mas nesta máquina só existe a instalação npm | caminho do instalador oficial **não verificado** aqui |
| Snapshot velho | medido na seção anterior | o Dish avisa e oferece reabrir; não promete atualização |

## Fases

1. **Fase 1 (implementada)** — adaptador somente leitura, contrato acima e testes
   contra um SDK falso que esconde segredos em campos proibidos.
2. **Fase 2 (implementada)** — seção **Provedores** nas configurações: catálogo
   com estado, origem e contagem de modelos, primeiro uso e "Verificar de novo".
   A leitura roda na `background_executor` com prazo de 20s; falha vira uma linha
   em português, nunca lista vazia nem pânico.
3. **Fase 3 (implementada)** — escrever credencial pelo adaptador: chave de API,
   login por conta do provedor (avisos `auth_url`, `device_code` e `progress`),
   cancelamento, remoção com confirmação e releitura do estado depois de cada
   ação. Sem UI otimista: a lista só muda quando o Pi confirma.
4. **Fase 3b (implementada; corrigida pelo PR 1)** — depois de um login ou
   remoção, as conversas abertas releem `get_available_models` e
   `get_available_thinking_levels`. O PR 1 acrescenta a comparação honesta com o
   adaptador e a ação **"Reabrir esta conversa"**. A releitura sozinha não
   atualiza o snapshot de um processo já iniciado.
5. **Fase 4** — provedores locais/compatíveis (`models.json`), preservando o
   arquivo existente. Separado de propósito: não bloqueia a fase 3.
6. **Fase 5** — pedir ao Pi `pi auth login|logout` e aposentar o adaptador, se
   ele preferir expor isso na CLI. Até lá, o adaptador é a ponte.

## Limites conhecidos

- **O campo de segredo é mascarado, sem botão de revelar.** Perguntas
  `secret` desenham `•` por caractere; o valor real fica só no buffer e vai no
  envio. Não há `reveal` nesta etapa. Em compensação ela não é copiada para o
  estado, não entra em registro, não volta pelo protocolo e o campo é limpo no
  instante do envio.
- **`getDeviceId` não é enviado.** Alguns fluxos (o host id da OpenAI) aceitam
  um identificador estável da instalação; o Dish ainda não cria nem guarda um,
  para não introduzir um identificador persistente sem decisão explícita.
- **Provedores só-OAuth funcionam; provedores sem login interativo não mostram
  ação.** Numa instalação normal, 41 dos 42 provedores têm login por chave e 9
  têm OAuth, então não há caso vazio na prática.
- **Reabrir exige uma sessão salva.** Conversa sem arquivo ainda não pode ser
  reaberta; a ação fica desabilitada e o motivo é explicado.
- **Reabrir não interrompe trabalho.** Com streaming, fila, compactação ou
  diálogo pendente, a ação fica desabilitada; o Dish não aborta nada para
  "consertar" o catálogo.

## Como verificar

```sh
# contrato, contra o SDK falso (sem Pi, sem rede, sem credencial)
python3 -m unittest discover -s tests -p 'test_provider_adapter.py' -v
python3 -m unittest discover -s tests -p 'test_provider_login.py' -v

# contra o Pi instalado, só estado e origem
node scripts/pi-auth-adapter.mjs --status | jq '.data.providers[] | select(.configured)'

# smoke de UI (Xvfb). O último caso usa o `pi` real do PATH com agente
# isolado e chaves falsas; sem `pi`, ele anuncia que foi ignorado.
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/providers_smoke.py
```

O smoke cobre, com Pi e SDK sintéticos, o catálogo, o primeiro uso, a falha
honesta, o login/remoção por chave e o fluxo completo de primeiro uso. O caso
real reproduz o `Model not found` do snapshot de startup, verifica o aviso
honesto, reabre a conversa e confirma que rascunho e histórico continuam lá.

Privacidade: `--status` lê o `auth.json` do usuário, como o Pi faria, e imprime
apenas estado e origem. O caso real do smoke usa `PI_CODING_AGENT_DIR` temporário
e nunca toca no agente do usuário.

## Log de comandos do reconhecimento

Nenhum comando de impressão de credencial foi executado. `pi auth print-api-key`
e `print-bearer-token` foram consultados **apenas com `--help`**.

```sh
pi --version                                   # 1.4.0
pi --help                                      # install/remove/uninstall/update/list/config/auth/mcp
pi auth --help                                 # só print-api-key, print-bearer-token, check
pi auth check --provider {deepseek,anthropic,openai-codex,opencode-go} --json --no-refresh
pi --list-models | wc -l                       # 37 (36 modelos, 2 provedores)

node scripts/pi-auth-adapter.mjs --status      # 42 provedores, 41 com chave, 9 com OAuth
node scripts/pi-auth-adapter.mjs --status --agent-dir <tmp>   # cria auth.json {} 600

# precedência (SDK, diretório isolado, valores falsos)
node /tmp/precedence-test.mjs <auth.json> <models.json|-> deepseek <rótulo>   # 9 casos

# snapshot do Pi em modo RPC (diretório isolado, chaves falsas)
python3 /tmp/rpc-staleness.py     # lista não muda após login externo
python3 /tmp/rpc-stale2.py        # "Model not found" após login externo
python3 /tmp/rpc-stale3.py        # "No API key" após logout externo
```

`~/.pi/` não foi alterado no reconhecimento: os testes de credencial usaram
`mktemp -d` e `PI_CODING_AGENT_DIR`, e contra o agente real o adaptador apenas
leu estado/origem (nenhuma credencial foi gravada ou removida).
