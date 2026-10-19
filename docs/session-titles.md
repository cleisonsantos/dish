# Títulos de sessão

Com **Configurações → Aplicativo → Títulos gerados por modelo** ativado, o Dish
pede ao modelo da sessão um nome curto para o **primeiro pedido relevante**.
A geração ocorre após `agent_settled`, fora da thread da interface, e usa tokens.
Não é uma declaração de que a tarefa terminou com sucesso.

## Identidade e persistência

- O título identifica a tarefa inicial. Pedidos posteriores não mudam um nome
  já definido, evitando renomeações repetidas e chamadas extras ao modelo.
- Qualquer nome não vazio informado pelo Pi é preservado, inclusive nomes
  manuais, nomes de extensões e títulos gerados numa abertura anterior. Não há
  metadado confiável para distinguir esses casos após reabrir a sessão.
- O processo isolado não acessa a conversa principal. O resultado é salvo pelo
  comando RPC `set_session_name`, no escritor já existente; o Dish não edita o
  arquivo de sessão. A interface aguarda a confirmação/evento do Pi.
- Não há migração em massa nem geração automática ao carregar históricos.
  Uma sessão antiga sem nome pode receber um título após uma nova rodada.
- Uma geração atrasada é descartada se a sessão mudar, houver renomeação manual,
  a preferência mudar ou uma execução estiver em andamento. Neste último caso,
  uma rodada posterior pode tentar novamente.

## Chamada e falhas

A chamada usa `--print --mode json --no-session`, sem ferramentas, extensões,
MCP, skills, templates ou arquivos de contexto. Usa um system prompt específico
para títulos e `--thinking off` (sujeito às capacidades do modelo), em vez de
herdar um prompt de programação e o nível de raciocínio configurado.

Somente texto de `message_end` de assistente com `stopReason: stop` é aceito;
raciocínio não entra no título. A resposta deve conter uma única linha válida.
A captura é limitada a 64 KiB, com prazo de 30 segundos incluindo a espera pelo
encerramento do processo. stdout/stderr do provedor não são exibidos nem logados.

Falhas de inicialização, processo, leitura, timeout ou formato produzem um aviso
em português. O trecho do pedido continua sendo apenas o fallback visual, nunca
um nome gerado persistido. A tentativa não se repete a cada rodada para o mesmo
pedido; desativar e reativar a opção permite tentar novamente. Falhas ao salvar o
nome também geram aviso sem converter a conversa principal em estado de erro.

## Diagnóstico da #86

A implementação anterior confirmadamente mantinha os resumos apenas em memória
e usava fallback literal silencioso em qualquer falha. Também herdava o system
prompt e o esforço padrão e aceitava somente uma linha de stdout textual. Esses
caminhos explicam como um recorte pode aparecer, mas a falha específica da
instalação relatada não foi reproduzida com provedor real: os testes usam somente
dados e respostas sintéticas, sem rede nem credenciais.

## Testes

- Unitários em `src/demand.rs`: argumentos isolados, leitura JSON, exclusão de
  thinking, erro de processo, formato inválido, timeout de leitura e processo que
  fecha stdout mas não encerra.
- `tests/session_titles_smoke.py`: geração semântica, persistência pelo Pi
  sintético, reabertura, estabilidade após novos pedidos, renomeação manual com
  geração em voo e aviso em falha, sem vazar diagnóstico privado.

```sh
DISH_TEST_BIN=/caminho/para/dish xvfb-run -a -s '-screen 0 1280x960x24' \
  python3 tests/session_titles_smoke.py
```
