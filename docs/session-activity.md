# Atividade e atenção na lista de sessões (#21)

Dish separa **atividade**, **atenção** e **leitura**. Os sinais coexistem:
uma resposta pode estar não lida enquanto outra execução já está em andamento.
Nenhum indicador significa que a tarefa foi concluída com sucesso.

## Origem dos sinais

Os registros chegam pelo leitor único de `src/rpc.rs` (`pi --mode rpc`);
`src/state.rs` interpreta os eventos, e `src/session_activity.rs` contém o
modelo de filtros e o contador testável de respostas ao vivo.

| Sinal | Origem | Limitação / significado |
| --- | --- | --- |
| Executando | `get_state.isStreaming`, `agent_start`/`agent_end`, `agent_settled`, streaming, compactação e execução bash local já rastreada pelo Dish | Só processos conhecidos pelo Dish. `turn_end` não encerra por si só o agente entre turnos. Um diálogo isolado não implica execução. |
| Precisa de você | `extension_ui_request` que cria um modal (`select`, `confirm`, `input`, `editor`) | Notificações/widgets/status de extensão não criam este sinal. O modal desaparece pela resposta ou cancelamento já implementados. |
| Resposta nova | `message_end` de assistente com texto ou de resultado de ferramenta, seguido por `agent_end`, enquanto a conversa não está visível numa janela ativa | `get_messages` não cria respostas novas. `agent_end` vazio ou apenas chamadas de ferramenta não bastam. Texto de execução interrompida ou com erro pode ser não lido, mas nunca vira indicação de sucesso. |
| Erro | resposta RPC `success: false`, `stopReason: error`, `turn_end.errorMessage`, `tool_execution_end.isError`, `extension_error`, falha de compactação/retry ou fechamento do transporte | Não inferido de texto de stderr nem do código de um comando não reportado pelo Pi. É limpo no próximo `agent_start`; o transcript preserva os detalhes. |
| Interrompida | `stopReason: aborted` ou compactação explicitamente abortada | Não é erro. Uma solicitação de abortar não basta: o estado vem do evento. |
| Em fila | `queue_update.steering` e `queue_update.followUp`, além das filas já mantidas pelo composer | Separado de diálogos que exigem ação. O protocolo reconcilia a fila existente. |
| Inativa | Conversa aberta sem os sinais acima | Não implica sucesso, nem autoriza iniciar trabalho automaticamente. |
| Salva | Catálogo de arquivos JSONL, sem conversa/processo aberto correspondente | Ler o catálogo não inicia Pi. Uma sessão salva pode ter resposta não lida persistida. |

`agent_active` mantém o sinal de execução entre turnos. O fechamento do transporte
limpa a execução e informa erro. A regra de confirmação ao fechar sessões continua
considerando tanto execução quanto diálogo pendente.

## Horário e ordenação

- Em sessões ao vivo, o horário de atividade é **observado pelo cliente** nos
  limites de atividade (início/fim de agente/ferramenta/compactação, diálogo e fila).
  Não é apresentado como timestamp original de execução.
- Antes de haver atividade ao vivo, usa-se o horário do último prompt já conhecido,
  o mtime do arquivo salvo ou a criação da conversa nesta janela. Para resposta
  não lida salva, usa-se o horário observado persistido.
- O resumo relativo em português (`agora`, `há … min/h/d`) informa recência, não
  duração ou sucesso. Timestamps completos do transcript pertencem à #23.
- Agrupamento por projeto; dentro do projeto, atividade mais recente primeiro,
  com desempate determinístico pela identidade da linha.
- Deltas de streaming não atualizam a ordenação. Enquanto o ponteiro está na
  navegação ou a busca tem foco, a ordem visível é mantida; linhas novas entram
  depois das existentes. Seleção da conversa não depende de sua posição visual.

## Leitura e persistência

Uma resposta é lida ao selecionar/abrir a conversa, ou quando a conversa ativa
está visível em uma janela ativa. Configurações e confirmação de fechamento não
contam como leitura. Abrir uma sessão salva não lida marca-a como lida.

`unread_sessions` guarda o caminho canônico do arquivo e o horário observado de
conclusão nas preferências do Dish: `$XDG_CONFIG_HOME/dish/state.json`, com fallback
para `$HOME/.config/dish/state.json`. O filtro selecionado também é persistido.
A gravação usa o escritor único de preferências existente, fora da thread de UI.
Nenhum arquivo de sessão do Pi é alterado.

Uma conversa sem arquivo informado pelo Pi mantém leitura apenas nesta janela:
sem identidade persistível não se inventa uma sessão no histórico. Respostas
criadas por processos externos, ou durante o período em que Dish está fechado,
não são inferidas como não lidas pelo mtime.

## Interface e teclado

- Filtros: **Todas · Precisa de você · Não lidas · Executando**.
- Texto e ícones de atenção/leitura complementam as cores. Respostas novas usam
  ícone de arquivo, não um check de sucesso.
- Projetos recolhidos exibem contadores de pendências e respostas novas. A faixa
  recolhida usa `P` (pendências) e `N` (novas), além de ícones por conversa.
- `Ctrl+K`: busca; `↑`/`↓`: linha com contorno de foco e rolagem; `Enter`: abrir.
  `←`/`→` recolhem/reabrem o projeto destacado. Uma nova busca ou mudança de
  filtro revela os projetos correspondentes; recolher manualmente continua possível.
- `Ctrl+Alt+F`: alternar filtros e focar busca. `Ctrl+L`: voltar ao composer ou
  diálogo ativo. `Ctrl+Tab` / `Ctrl+Shift+Tab`: conversas abertas.
- Os filtros quebram linha em larguras menores; estados podem quebrar linha.

## Validação

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
cargo build --release
DISH_TEST_BIN="$PWD/target/release/dish" xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/desktop_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/session_activity_smoke.py
```

Os testes usam Pi sintético, projetos/arquivos temporários, sem rede, credenciais
ou sessões reais. O fixture usa barreiras de conclusão liberadas pelo teste,
com prazo limitado, para a navegação em segundo plano não depender de sleeps. O smoke específico cobre filtros vazios, execução em segundo
plano, fila, resposta não lida com nova execução, diálogo pendente, erro,
interrupção, fechamento do transporte, término sem resposta, navegação por
teclado, contadores e reinício com XDG/layout estreito. A CI executa este smoke
contra o binário extraído do pacote candidato.
