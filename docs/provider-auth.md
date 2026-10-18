# Provedores e autenticação (#69) — contrato do adaptador

Registro da fase 1: prova de conceito e contrato de compatibilidade e segurança.
Nada aqui muda a interface; a seção **Provedores** nas configurações é a fase 2.

O Dish não autentica nada. O Pi é dono do OAuth, do refresh e do armazenamento;
o Dish lê estado e mostra, e quando chegar a hora de escrever, escreve pelo Pi.
Este documento fixa como essa fronteira funciona e o que foi medido contra o Pi
real, para que a implementação não dependa de suposições.

## O problema

Um usuário novo pode terminar a instalação do Pi e do Dish sem nenhum modelo
disponível, e sem um caminho pela interface para entender o motivo. Medido:
`get_available_models` no RPC devolve **0 modelos** quando não há credencial —
o RPC não expõe o catálogo de provedores, só o que já está utilizável. Também
não existe `login`/`logout` no RPC, e mandar `/login` como prompt está fora de
questão (a issue é explícita: não enviar segredo nem comando de TUI ao agente).

## O que o Pi oferece hoje (medido contra o Pi 1.2.0)

| Superfície | O que dá | O que não dá |
| --- | --- | --- |
| RPC (`get_available_models`) | modelos dos provedores já utilizáveis | catálogo sem credencial, login, logout |
| `pi auth check --provider P --json [--no-refresh]` | `{status, provider, reason}`; `ready` = exit 0, `not_ready` = 1, `invalid` = 2 | listar provedores (exige `--provider` ou `--model`) |
| `pi auth print-api-key` / `print-bearer-token` | o segredo em texto puro | — **proibido para o Dish** |
| SDK (`ModelRuntime`) | 42 provedores sem credencial, estado e origem, `login`/`logout` | exige um runtime JS |

A tabela de ambiente por provedor (mais de 30 variáveis) está em
`docs/providers.md` do próprio Pi.

### O que o SDK devolve, sem segredo

Medido com um diretório de agente isolado (nenhuma credencial) e com o
`auth.json` real:

- `getProviders()` → **42 provedores** com o catálogo completo, mesmo sem
  credencial nenhuma. É exatamente o que o RPC não entrega.
- `getModels(id).length` → quantidade de modelos por provedor (`amazon-bedrock`
  193, `azure` 45, `openai` 44, `anthropic` 17, …).
- `getProviderAuthStatus(id)` → `{configured, source, label}`, com
  `source` ∈ `stored · runtime · environment · fallback · models_json_key ·
  models_json_command`. É a **origem** que a issue pede, sem valor nenhum.
- `listCredentials()` → `[{providerId, type}]`, com `type` ∈ `api_key · oauth`.
- `login(providerId, type, interaction)` e `logout(providerId)`.

O protocolo de interação do login é pequeno e fechado, e é o que a interface
precisa saber apresentar:

- `prompt` — `text`, `secret`, `select` (com opções id/rótulo/descrição) e
  `manual_code` (para colar o código de um redirecionamento OAuth).
- `notify` — `info` (com links), `auth_url`, `device_code` (código, URL,
  intervalo, validade) e `progress`.

## Armadilhas medidas (o contrato de compatibilidade)

1. **`refreshOnCreate: false` faz os acessores síncronos mentirem.** Com
   `false`, `getProviderAuthStatus("openai")` devolve `{configured: false}`,
   `hasConfiguredAuth` devolve `false` e `isUsingOAuth` devolve `false` — mesmo
   com a credencial `openai` presente em `auth.json`, como `listCredentials`
   confirma. Com o padrão (`true`) os três passam a responder a verdade:
   `{configured: true, source: "stored"}`. O adaptador nunca cria o runtime com
   `false`, e nenhuma decisão de interface pode se basear em status síncrono
   vindo de um runtime preguiçoso.
2. **`models-store.json` não é catálogo de provedores.** Ele continha apenas os
   dois provedores configurados nesta máquina, apesar de existir um catálogo de
   42 no SDK. Não ler esse arquivo como fonte de verdade.
3. **`checkAuth()` verifica disponibilidade** e pode sair para a rede. Por isso
   o adaptador responde *estado* pelas funções síncronas e deixa `checkAuth`
   para uma ação explícita de "verificar" com timeout e cancelamento.
4. **`getAuth()` devolve o segredo** em `.auth`. O adaptador nunca o chama;
   `listCredentials` e `getProviderAuthStatus` bastam para tudo que é leitura.
5. **O adaptador precisa de um runtime JS.** O Pi pode estar instalado como
   binário autocontido, e o Dish não embute Node. Quando não houver Node, a
   interface não pode fingir: ela diz que o estado não é verificável e aponta
   `/login` no Pi. O conserto definitivo é pedir ao Pi `pi auth login|logout`,
   que elimina a dependência de um runtime que o Dish não controla.

## Protocolo entre Dish e adaptador

`scripts/pi-auth-adapter.mjs`, um processo por consulta, JSONL em stdin/stdout.
Somente `status` na fase 1; `login` e `logout` respondem `not_implemented` de
propósito, e op desconhecida responde `unsupported_op`. Todos os erros são
códigos fechados (`package_not_found`, `package_invalid`, `probe_failed`,
`bad_json`), nunca texto de exceção cru com caminho de credencial.

```jsonc
// entrada
{"id": 1, "op": "status"}
// saída
{"id": 1, "ok": true, "data": {
  "pi": {"version": "1.2.0", "package": "…/@earendil-works/pi-coding-agent"},
  "providers": [{"id": "anthropic", "models": 17, "configured": false,
                 "source": null, "label": null}],
  "credentials": [{"providerId": "openai", "type": "oauth"}],
  "readOnly": true
}}
```

Localização do pacote do Pi, em ordem: `--package`, `DISH_PI_PACKAGE`, o prefixo
privado do Dish (`$XDG_DATA_HOME/dish/pi`), as releases do instalador oficial
(`~/.pi/agent/install/releases/*/node_modules/…`) e, por último, subir a partir
do executável do Pi. Um caminho explícito é **autoridade**: se não contém o
pacote, o adaptador falha em vez de usar outra instalação em silêncio.

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

## Fases

1. **Fase 1 (este PR)** — adaptador somente leitura, contrato acima e testes
   contra um SDK falso que esconde segredos em campos proibidos.
2. **Fase 2** — seção **Provedores** nas configurações: catálogo com estado,
   origem, contagem de modelos; linha "Conectar um provedor" quando não há
   nenhum pronto, em vez de o usuário descobrir ao enviar uma mensagem. Estado
   lido na `background_executor`, com timeout; falha vira uma linha em
   português, nunca um pânico. Nenhuma ação de escrita.
3. **Fase 3** — escrever credencial pelo adaptador: chave de API (prompt
   `secret`), login por navegador/código de dispositivo (`auth_url`,
   `device_code`), cancelamento, remoção com confirmação, e reconsulta do estado
   depois de cada ação. Sem UI otimista.
4. **Fase 4** — provedores locais/compatíveis (`models.json`), preservando o
   arquivo existente. Separado de propósito: não bloqueia a fase 3.
5. **Fase 5** — pedir ao Pi `pi auth login|logout` e aposentar o adaptador, se
   ele preferir expor isso na CLI. Até lá, o adaptador é a ponte.

## Como verificar

```sh
# contrato, contra o SDK falso (sem Pi, sem rede, sem credencial)
python3 -m unittest discover -s tests -p 'test_provider_adapter.py' -v

# contra o Pi instalado, só estado e origem
node scripts/pi-auth-adapter.mjs --status | jq '.data.providers[] | select(.configured)'
```

Privacidade: `--status` lê o `auth.json` do usuário, como o Pi faria, e imprime
apenas estado e origem. Ainda assim, ao colar a saída em um relatório, confira
que ela veio de `--status` e não de um comando de impressão de credencial.
