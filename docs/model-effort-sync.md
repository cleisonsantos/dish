# Troca de modelo e níveis de esforço

O Pi é a fonte do modelo ativo, do nível selecionado e das capacidades disponíveis.
O Dish não tem uma tabela própria de capacidades por modelo.

## Fluxo

A seleção pelo menu e `/model [provedor] modelo` usam a mesma transação:

1. Enviar `set_model` e aguardar a resposta com seu ID.
2. Após sucesso, ler `get_state` para obter o modelo e o nível autoritativos.
3. Após essa resposta, ler `get_available_thinking_levels`.
4. Só então liberar os controles e reconciliar o nível contra essa lista.

A inicialização, atualização após uma rodada e atualização do catálogo também
usam as consultas sequenciais de estado e capacidades. Nunca é escolhido um
fallback com a lista de outro modelo.

`src/model_sync.rs` mantém somente um comando dessa transação em voo. Trocas
rápidas são serializadas e escolhas ainda não enviadas são condensadas na última
escolha. Uma atualização de catálogo não apaga uma escolha explícita pendente.
Se outra escolha chegar durante uma consulta, os dados dessa consulta não são
aplicados. Respostas duplicadas/antigas são ignoradas pelo ID, inclusive erros.
Isso evita não só respostas obsoletas na UI, mas trocas assíncronas concorrentes
que poderiam mudar novamente o modelo no Pi após a última seleção.

## Interface e falhas

- Durante a transação: `atualizando esforço…`, sem expor a escada anterior.
- Lista vazia confirmada: `sem raciocínio`; não inventar opções fixas.
- Erro na troca: aviso em português e releitura automática do modelo efetivo no
  Pi. Isso cobre uma troca intermediária que funcionou antes de a última falhar.
- Erro na consulta ou lista malformada: `esforço indisponível` e aviso em português.
  Selecionar novamente o modelo permite tentar de novo.
- O nome do modelo não é alterado otimisticamente: muda com a confirmação do Pi.
- Comandos de esforço são bloqueados enquanto a lista estiver pendente.
- A sequência é assíncrona; não há espera bloqueante na thread de UI.

Não há um timeout novo para comandos de troca: se o Pi permanecer conectado mas
não responder, o controle permanece pendente. Não iniciamos uma segunda troca
concorrente para contornar isso, pois a primeira ainda poderia alterar o Pi depois.

## Evidência e cobertura

A #93 segue a #27/PR #29. A leitura do código anterior confirmou que `/model` não
atualizava as capacidades e que o seletor consultava níveis antes da confirmação.
A reprodução usa modelos sintéticos com escadas diferentes, não capacidades
presumidas de modelos reais:

- Testes puros de `ModelSync`: ordem das consultas, correlação de IDs, respostas
  obsoletas, escolhas rápidas, falha e recuperação.
- `tests/model_effort_smoke.py` + `tests/fixtures/effort_pi.py`: Pi assíncrono com
  confirmação bloqueada por barreira, listas diferentes, lista vazia, duplicação
  atrasada de capacidades e falhas. Exercita seletor e as duas formas de `/model`.

A troca específica entre os modelos reais mencionados pelo usuário ainda precisa
ser validada com os IDs e as listas efetivamente reportadas pelo Pi/provedor.
Não são necessários tokens nem sessões reais para os testes automatizados.
