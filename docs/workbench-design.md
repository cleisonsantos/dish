# Bancada de código

A primeira proposta mudou controles, mas preservou demais a composição antiga.
Esta revisão separa navegação recuada e conversa numa superfície única. O
protocolo, os arquivos do Pi e a origem das informações não mudam.

## Sistema visual

Tokens em `src/theme.rs`, compartilhados pelos componentes:

| Papel | Grafite |
| --- | --- |
| Navegação e detalhes | `#171B21` |
| Área de leitura | `#22272F` |
| Escrita e pedidos | `#2B323C` |
| Divisórias | `#39424F` |
| Texto | `#EDF0F4` |
| Texto secundário | `#AAB3BF` |

A rampa de esforço continua sendo a do Pi. Estados de execução, erro e conteúdo
novo mantêm cores, ícones e texto próprios. O esforço aparece como texto com o
nome do nível; não há equalizador, barras ou faixa de cor sobre o compositor.

Noto Sans na interface e na prosa; JetBrains Mono em código, comandos e valores
técnicos. Escala: 11,5 / 12 / 13 / 15 / 18 / 32 px. Fontes usam a
resolução/fallback do sistema.
O tamanho de 32 px só aparece no estado vazio, alinhado ao eixo da conversa.

## Composição

```text
navegação recuada   | superfície de conversa                  |
                   | título                         detalhes |
projetos e sessões |                        pedido do usuário |
                   | resposta do agente                       |
                   |                                          |
configurações      | compositor sem divisória interna          |
```

- Superfície principal recuada 8 px, com cantos contínuos.
- Cabeçalho, conversa e compositor usam a largura disponível, com margem interna
  fixa de 16 px; não há coluna central limitada a 760 px. Menus de esforço,
  comandos e modelos mantêm medidas próprias e o mesmo alinhamento lateral.
- A coluna central pode encolher verticalmente: na janela mínima, o histórico ou
  boas-vindas rola, sem cortar o compositor e o rodapé Git.
- Pedidos compactos à direita, limitados a 90% da área quando longos; respostas
  abertas à esquerda, sem cartões.
- Cantos pertencem ao fundo que os pinta: 14 px na moldura, 13 px no conteúdo
  dentro da borda de 1 px. Os detalhes arredondam só os cantos externos. O GPUI
  recorta descendentes em retângulo, não em curva; `overflow_hidden()` não
  substitui os raios dos fundos opacos. O painel sobreposto usa fundo escurecido
  e divisória, sem sombra invadindo os cantos da moldura.
- Cada turno e comando mostra seu marcador de tempo diretamente (início, fim
  ou resultado, com a fonte). O texto completo fica no tooltip e nos detalhes
  expansíveis. Ferramentas mantêm seus rótulos temporais explícitos: resultado
  registrado não vira duração de execução.
- Compositor tem uma única borda arredondada: nenhuma faixa reta de acento no topo
  e nenhuma divisória atravessando a área de escrita.
- Modelo e esforço ficam junto da escrita; enviar e interromper continuam sendo
  ações diferentes. Ativá-los devolve o foco ao editor.
- Configurações têm entrada nomeada no rodapé da navegação.

## Detalhes: comportamento preservado

O painel mantém o comportamento anterior ao redesign: 316 px, encaixado a partir
de 1120 px e sobreposto abaixo disso. Na primeira abertura do app, a largura
sugere seu estado; depois vale a escolha explícita do usuário. Ordem das seções:
sessão, Git, arquivos, contexto, turnos e ações.

A sobreposição usa `occlude()` na raiz, cobrindo corpo, cabeçalho e fundo. No GPUI,
um fundo pintado não impede que a lista atrás receba a mesma roda do mouse.
`overflow_y_scroll()` também não consome o evento por si só. O painel encaixado
é uma fronteira de rolagem explícita. Rolar além dos limites não move o histórico.

## Temas e diálogos

O catálogo tem quatro temas escuros e quatro claros; novos presets: Noite azul,
Ameixa, Lavanda e Sálvia. Aparência agrupa os temas por modo, incluindo os
personalizados. A escolha aplica a paleta imediatamente e persiste após reinício.
Veja [Temas do Dish](themes.md) para ids e formato JSON.

Configurações, confirmação de fechamento e diálogos de extensões usam scrims com
`occlude()` e fronteira de roda do mouse, sem impedir a rolagem dos descendentes.
Diálogos de seleção respeitam o espaço interno do scrim: cabeçalho longo e opções
rolam em áreas próprias. As linhas não são comprimidas para fazer a lista caber.
O foco recebe as teclas do diálogo; setas revelam a opção destacada por seu limite
real, inclusive na passagem da primeira para a última. A resposta preserva o id
recebido do Pi, que é a chave da pergunta pendente, não um novo id de comando.

## Movimento

`src/ui/motion.rs` concentra transições nativas de `AnimationExt`:

- Troca de conversa, menus, configurações e avisos: opacidade, 180 ms.
- Navegação: opacidade e deslocamento de 12 px, 220 ms.
- Detalhes abrem e fecham diretamente, como antes.
- Curva cúbica de desaceleração, sem salto ou overshoot.
- Indicadores repetidos existem apenas durante trabalho real informado pelo Pi.

A interpolação usa o relógio de quadros do GPUI; a cena usa seu backend gráfico.
Não há shaders de compute próprios, timers de animação ou promessa de FPS.
Transições de entrada param de pedir quadros ao terminar.

**Aparência > Reduzir movimento** usa `App::set_reduce_motion` e persiste em
`reduce_motion`. Sem escolha explícita, preserva a preferência do GPUI. Nesse
modo, transições mostram o estado final e indicadores ficam estáticos.

## Validação

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
cargo build --release --locked
xvfb-run -a -s '-screen 0 2080x1200x24' python3 tests/layout_smoke.py
xvfb-run -a -s '-screen 0 1680x1050x24' python3 tests/redesign_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/appearance_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/dialog_scroll_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/inspector_scroll_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/desktop_smoke.py
```

Testes unitários cobrem contraste, distinção entre estados e esforço, curva de
animação, breakpoint original dos detalhes e persistência da preferência de
movimento. Smokes usam Pi sintético e diretórios temporários. O teste de rolagem
primeiro exige que o histórico realmente se mova, depois verifica isolamento do
painel, inclusive nos limites, cabeçalho e fundo sobreposto; também exige que o
próprio painel continue rolando. O smoke de Aparência verifica pixels dominantes
de cada uma das oito paletas, gravação da escolha e reinício. O de diálogos cobre
listas longas na janela mínima, seleção por teclado e isolamento de rolagem de
configurações, seleção e confirmação de fechamento, nas duas direções.
O smoke de layout mede margens e largura real do compositor em 1600, 1920, 780
e 720 px, nos temas claro e escuro; confere os recortes e a curva da borda antes
e depois de abrir os detalhes, tanto encaixados quanto sobrepostos. Inclui
janela mínima de 720×520, navegação expandida com detalhes encaixados, pedidos
longos a 90% e uma resposta com marcador além dos antigos 760 px e texto final
quebrado em outra linha, para detectar limites e cortes indevidos.
