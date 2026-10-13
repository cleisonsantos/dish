# Temas do Dish

O Dish traz quatro temas embutidos — `Grafite` (escuro), `Papel frio`
(claro), `Papel quente` (claro) e `Grafite baixo` (escuro de baixo
contraste) — e aceita temas seus em JSON, escolhidos na seção
**Aparência** das configurações.

## Onde os arquivos ficam

| Sistema | Diretório |
| --- | --- |
| Linux / macOS | `$XDG_CONFIG_HOME/dish/themes/` (padrão `~/.config/dish/themes/`) |
| Windows | `%APPDATA%\dish\themes\` |

Um arquivo `.json` por tema. O botão **Abrir pasta de temas** na seção
Aparência cria o diretório se preciso e o abre no gerenciador de arquivos.

Os temas são lidos **quando o Dish abre**. Depois de editar ou criar um
arquivo, feche e abra o Dish de novo para vê-lo na lista.

## Formato

```json
{
  "$schema": "https://raw.githubusercontent.com/cleisonsantos/dish/main/docs/theme-v1.schema.json",
  "name": "Meu tema",
  "appearance": "dark",
  "colors": {
    "bg": "#0c0e12",
    "surface": "#101318",
    "surface2": "#171b21",
    "inset": "#08090d",
    "hover": "#1b1f26",
    "line": "#22262e",
    "lineSoft": "#191d23",
    "lineStrong": "#2e343d",
    "text": "#e9ebee",
    "dim": "#a6aeb8",
    "faint": "#858d97",
    "onAccent": "#0c0e12",
    "running": "#4fb3a5",
    "failure": "#cf5f92",
    "warn": "#d8a24a",
    "attention": "#2ecc71",
    "ok": "#a6aeb8",
    "gitAdded": "#81b88b",
    "gitRemoved": "#d98282"
  }
}
```

- `name`: o nome na lista. Sem ele, vale o nome do arquivo.
- `appearance`: `dark` ou `light` (padrão `dark`). Define de qual tema
  embutido vêm as cores que você não declarar e qual rampa de esforço é
  usada — como no tema escuro do Pi, os tons de raciocínio são os mesmos
  do `Grafite`; no claro, do `Papel frio`.
- `colors`: as cores, em `#rrggbb` ou `#rrggbbaa`.
- `$schema`: opcional; serve para o editor validar e completar o arquivo.
  O esquema está em [`docs/theme-v1.schema.json`](theme-v1.schema.json).

### Papéis das cores

| Chave | Papel |
| --- | --- |
| `bg` | Fundo da janela, atrás da conversa. |
| `surface` | Painéis: navegação, inspetor, cartões flutuantes. |
| `surface2` | Superfícies elevadas: compositor, pedido do usuário, linha ativa. |
| `inset` | Poços: código, saída de ferramenta, campos de busca. |
| `hover` | Realce de linha sob o mouse ou teclado. |
| `line`, `lineSoft`, `lineStrong` | Fios, do mais sutil ao mais firme. |
| `text`, `dim`, `faint` | Texto principal, secundário e metadados. |
| `onAccent` | Texto sobre o accent (o botão de enviar, por exemplo). |
| `running` | Execução em andamento. |
| `failure` | Falha. Não pode parecer o coral do esforço máximo. |
| `warn` | Aviso de recurso (contexto cheio). |
| `attention` | Resposta nova ainda não lida. |
| `ok` | Sucesso: neutro de propósito. |
| `gitAdded`, `gitRemoved` | Diff do rodapé de Git. |

A rampa de esforço (`off`…`max`) acompanha o tema base e **não** é
configurável nesta versão.

## Validação e erros

- Chave ausente em `colors`: o valor do tema base da `appearance`.
- JSON inválido, cor fora de `#rrggbb`/`#rrggbbaa` ou `appearance`
  desconhecida: o arquivo é ignorado e um aviso na interface nomeia o
  arquivo e o problema. O Dish abre normalmente.
- Chaves desconhecidas são ignoradas, para o formato poder crescer.

O Dish não verifica contraste nem a separação entre cores de status de
temas do usuário — os testes automáticos cobrem apenas os embutidos.
Confira o resultado nas duas superfícies em que cada cor aparece: nos
temas claros, por exemplo, `text`, `dim` e `faint` precisam de 4,5:1
sobre `bg`, `surface`, `surface2` e `inset`.
