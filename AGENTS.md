# AGENTS.md — acordo de trabalho para agentes concorrentes

Este arquivo é o contrato mínimo para qualquer agente (humano ou IA) que edite o
Dish em paralelo com outros. Leia antes de escrever código. O restante — build,
testes, licenças, release, estilo — está em [`CLAUDE.md`](CLAUDE.md).

O problema que ele resolve: o histórico usa uma identidade noreply compartilhada,
então commits não dizem quem escreveu nem de qual checkout saíram. Worktree e
branch declarados são a única forma de reconstruir isso.

## 1. Um worktree por agente/tarefa

```sh
git worktree add ../dish-issue<N> -b <tipo>/<N>-<slug> origin/main
```

- **Nunca dois agentes no mesmo working tree.** Um worktree, um escritor. Isso
  vale também para o checkout principal (`~/dev/dish`): ele é somente leitura
  para trabalho de tarefa.
- Antes de começar, confirme onde você está: `git rev-parse --show-toplevel` e
  `git status -sb`. Se o caminho não for o seu worktree, pare e mova o trabalho.
- Ao terminar (depois do merge), remova o seu: `git worktree remove <caminho>`.
  Nunca remova worktree de outro agente.

## 2. Convenção de branch

`<tipo>/<issue>-<slug>`, minúsculas, hífens, sem acentos.

- `tipo`: `feat`, `fix`, `docs`, `design`, `chore`, `refactor`, `test`.
- `<issue>`: número da issue quando existir; sem issue, use um slug curto.
- Exemplos: `docs/20-agents-md`, `fix/103-pi-discovery`, `feat/59-first-run-wizard`.

Caminho do worktree acompanha o número: `~/dev/dish-issue<N>`. Sem issue, use
`~/dev/dish-<slug>`.

## 3. `main` só muda por pull request

- Não faça push direto em `main`, nem force-push em branch compartilhada.
- Traga atualizações com `git fetch origin && git rebase origin/main` **na sua
  branch**, nunca mexendo no checkout principal.
- Um PR por tarefa. Se outro agente estiver ativo, `main` recebe mudanças apenas
  via PR revisado.

## 4. Declare worktree e branch

Toda descrição de PR e todo commit de merge devem dizer de onde a mudança veio:

```
Worktree: ~/dev/dish-issue20
Branch: docs/20-agents-md
```

No commit, use o rodapé:

```
Worktree: ~/dev/dish-issue20
Branch: docs/20-agents-md
```

Sem isso o revisor não consegue distinguir dois trabalhos simultâneos.

## 5. Trabalho não commitado que não é seu

Se encontrar alterações não commitadas que você não fez (checkout compartilhado,
worktree de outro agente, branch inesperada):

- **Não** sobrescreva, não faça `stash`, `reset`, `checkout --`, `clean` nem
  reverta. Nem "só para limpar".
- **Reporte**: caminho, `git status --short`, branch atual e, se possível, quem
  parece ser o dono. Siga a tarefa no seu próprio worktree.
- Se o trabalho alheio bloquear a sua tarefa, pare e pergunte em vez de resolver
  por conta própria.

## 6. Referência

- Issue #20 — este acordo.
- Issue #18 — exibir worktree, branch e status na interface, para que o contexto
  git da sessão não dependa de memória.
