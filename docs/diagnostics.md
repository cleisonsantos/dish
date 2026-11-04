# Diagnóstico

O launcher descarta o stderr do Dish, então uma queda não deixava rastro. Desde
a issue #147 o aplicativo grava um relatório de pânico em `crash.log` e os
avisos de `logging::warn` também em `dish.log`. Os dois arquivos ficam no
diretório de estado do usuário, com permissão privada (0700 no diretório,
0600 nos arquivos, no Unix).

## Onde ficam

| Sistema | Diretório |
| --- | --- |
| Linux, com `XDG_STATE_HOME` definido | `$XDG_STATE_HOME/dish/` |
| Linux, sem `XDG_STATE_HOME` | `~/.local/share/dish/logs/` |
| Windows | `%LOCALAPPDATA%\dish\logs\` |

Em cada diretório:

| Arquivo | O que é |
| --- | --- |
| `crash.log` | Relatório do pânico mais recente (acumula enquanto couber). |
| `crash.log.1` | Relatório da execução anterior. |
| `dish.log` | Avisos de diagnóstico com timestamp UTC. |
| `dish.log.1` | `dish.log` anterior, rodado por tamanho. |

A precedência é o primeiro valor não-vazio: `XDG_STATE_HOME` manda mesmo que o
diretório ainda não exista (o Dish o cria). No código, `logging::log_dir()` é a
fonte de verdade e `logging::log_file_path()` / `logging::crash_log_path()`
devolvem os arquivos escolhidos.

## Limites e rotação

- **`dish.log`**: 256 KiB. Antes de anexar uma linha, se o arquivo já chegou ao
  teto, ele vira `dish.log.1` (o anterior é descartado) e um novo `dish.log`
  começa. A checagem acontece a cada aviso.
- **`crash.log`**: 64 KiB no total do arquivo. O relatório novo é aparado ao
  espaço que resta; se o arquivo já gastou o orçamento, nada é gravado.
- **Uma rotação de `crash.log` por execução**: no início do `main`,
  `crash.log` vira `crash.log.1`, sobrescrevendo o `.1` anterior. Para guardar
  várias quedas, **copie `crash.log` antes de reabrir o Dish**, porque a
  próxima execução empurra o conteúdo para `.1` e a seguinte o substitui.

## O que é redigido

Nada chega ao disco sem passar por `logging::redact`:

- o valor de `PI_CODING_AGENT_DIR` vira `<pi-agent>`;
- o valor de `HOME` vira `~`;
- palavras que começam com `sk-` viram `<redigido>`;
- campos sensíveis (`token=`, `key=`, `secret=`, `password=`,
  `authorization:`, sem diferenciar maiúsculas) viram
  `nome=<redigido>`, preservando o nome para o leitor saber o que faltou;
- caminhos com `/sessions/` ou terminados em `.jsonl` viram `<sessão>`;
- a mensagem e as linhas de aviso são cortadas em 2048 bytes, com
  ` …[truncado]`;
- o campo `local:` do relatório de pânico passa pela mesma redação, sem corte,
  então um caminho absoluto em `$HOME/.cargo` vira `~/.cargo/…`.

O backtrace não sofre o corte de 2048 bytes (seria ilegível), mas o relatório
inteiro respeita o teto de 64 KiB.

## Desligar o `dish.log`

`DISH_LOG_FILE=0 dish` mantém apenas o stderr e o `crash.log`. Útil quando o
launcher redireciona o stderr e você não quer arquivo de avisos; o relatório de
pânico continua sendo gravado.

## Backtrace no build de release

O build de release é remapeado e sem debuginfo: o backtrace mostra nomes de
função, sem arquivo nem linha. É por isso que o relatório traz a linha fixa
`símbolos: build sem debuginfo — o backtrace mostra funções, sem arquivo/linha`.
A localização exata do `panic!` ainda aparece em `local:` (arquivo, linha e
coluna do código-fonte).

Para investigar ao vivo, rode no terminal, sem launcher:

```sh
RUST_BACKTRACE=1 dish-pi   # pacote .deb
RUST_BACKTRACE=1 dish      # tarball/AppImage
```

O hook do Dish já captura o backtrace sozinho; a variável não muda o relatório,
mas o terminal garante que o stderr não seja descartado. Em build de
desenvolvimento (`cargo run`) as linhas de arquivo aparecem porque o debuginfo
existe.

## Verificação automática

`tests/diagnostic_privacy_smoke.py` provoca um pânico sintético
(`DISH_TEST_PANIC`, só para testes) e confere que `crash.log` contém o
relatório, respeita 64 KiB e não vaza o segredo, o home nem o caminho da
sessão. A segunda fase abre uma janela e confere o `dish.log` da mesma forma.
