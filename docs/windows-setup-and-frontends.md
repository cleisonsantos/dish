# Windows: setup no app/instalador e survey de clientes (#15)

**Status:** avaliação escrita; nenhum código alterado.

**Base:** branch `docs/15-windows-setup-survey`, `origin/main` em `51a753e`
(2026-10-27).

**Método:** cada afirmação sobre o Dish aponta arquivo e linha verificados no
worktree. As fontes externas foram consultadas em 2026-10-27 e estão listadas no
fim. Nenhum instalador foi executado e não houve teste de runtime no Windows;
as conclusões sobre o NSIS e os scripts PowerShell vêm da leitura do código.
Referências curtas (`nsi:`, `installation.rs:`, `startup.rs:`) apontam para
`scripts/windows-installer.nsi`, `src/installation.rs` e `src/startup.rs`; o
inventário abaixo traz os caminhos completos.

## Resumo

1. O NSIS **não instala Pi nem Node**. Ele instala o app, cria atalhos e
   desinstalador, e, opcionalmente, entrega o Pi a um helper PowerShell que
   baixa o instalador oficial e exige revisão e a palavra `INSTALAR`
   (`scripts/windows-installer.nsi:95-101`, `145-151`,
   `scripts/install-pi-windows.ps1:8-24`). A premissa da issue #13 — instalar
   Pi/Node dentro do wizard — não está implementada no `.nsi`.
2. O wizard do app já resolve descoberta, validação, escolha, instalação via
   npm e persistência da escolha no Windows (`src/installation.rs:48-55`,
   `193-228`, `340-391`; `src/startup.rs:119-172`, `176-245`). O que falta é
   instalar o Node e oferecer um caminho de instalador oficial que funcione no
   Windows: o botão atual chama uma função que recusa qualquer sistema que não
   seja Linux (`src/installation.rs:284-286`).
3. Há sobreposição real em três pontos: detecção de Pi com regras diferentes
   (`where pi` no NSIS versus busca ampliada no app), dois caminhos de
   instalação de Pi com UX diferente, e duas noções de versão (`0.1.0` no
   binário, `0.1.0-alpha.5` no rótulo do pacote).
4. A divisão limpa é: **NSIS só empacota o Dish**; o **app é dono dos
   pré-requisitos** (detectar, validar, instalar, lembrar); o **Pi é dono da
   própria instalação/atualização**. Atualização do Dish continua sendo o
   desenho do #57, hoje não implementado (`docs/update-design.md:3-5`).
5. O survey mostra um padrão: apps desktop comparados empacotam o próprio
   runtime e se atualizam sozinhos; onboarding de dependência aparece nos
   instaladores de terminal (Pi, Claude Code, Codex, opencode). O instalador
   oficial do Pi no Windows já cobre as dependências que o #13 pede, mas de
   forma interativa e em terminal (`pi.dev/install.ps1`, linhas 600-657,
   759-790, 1966-1972).

# Parte 1 — Quanto do fluxo cabe no app/instalador

## 1.1 O que existe hoje

| Peça | O que faz | Evidência |
| --- | --- | --- |
| `.github/workflows/windows.yml` | Build nativo, checagem do subsistema GUI, assinatura opt-in, empacotamento NSIS, artifacts e publicação no Release | `windows.yml:40-49` (subsistema), `50-55` e `98-103` (assinatura), `96-103` (pacote), `121-161` (publish) |
| `scripts/windows-installer.nsi` | Instalação por usuário em `%LOCALAPPDATA%`, licença, páginas, detecção de Pi, atalhos, desinstalador | `nsi:37-42`, `55-61`, `75-116`, `159-190`, `203-222` |
| `scripts/install-pi-windows.ps1` | Baixa `https://pi.dev/install.ps1`, mostra SHA-256/Authenticode, abre no Notepad, exige `INSTALAR`, recheca o hash e executa | `install-pi-windows.ps1:8-24` |
| `scripts/sign-windows.ps1` | Assina e verifica os artefatos quando os dois secrets existem; sem secrets, segue sem assinar | `sign-windows.ps1:3-6` |
| `build.rs` | Embute ícone e metadados no `dish.exe`; versões vêm de `CARGO_PKG_VERSION` | `build.rs:11-31` |
| `src/installation.rs` | Descoberta de Pi, validação de versão, checagem de Node/npm, instalação npm privada, comando manual | `installation.rs:48-55`, `193-228`, `340-391`, `394-400` |
| `src/startup.rs` | Wizard de primeira execução: verificar, escolher, instalar, concluir | `startup.rs:19-35`, `119-257`, `415-533` |
| `docs/update-design.md` | Desenho de atualização do Dish; explicitamente sem implementação | `update-design.md:3-5`, `134-146` |

## 1.2 O que o wizard já resolve no Windows

O fluxo em `src/startup.rs` é acionado na primeira execução (ou quando o Pi
some). No Windows ele já faz:

1. **Descoberta ampliada.** `installation::program()` tenta, nesta ordem:
   `DISH_PI_BIN`, preferência salva, `pi`/`pi.cmd` no `PATH`, e o prefixo
   privado do Dish (`installation.rs:48-55`). Sem resultado no `PATH`,
   `find_installed()` procura em `%USERPROFILE%\.local\bin`,
   `%USERPROFILE%\.npm-global\bin`, `%USERPROFILE%\.pi\agent\bin`,
   `%APPDATA%\npm`, `%LOCALAPPDATA%\Programs\pi` e `%LOCALAPPDATA%\pi`
   (`installation.rs:193-228`). Isso cobre o caminho usado pelo instalador
   oficial do Pi (`%USERPROFILE%\.pi\agent\bin\pi.cmd`; `pi.dev/install.ps1`,
   linhas 2020-2024, na versão consultada).
2. **Validação.** Executa `pi --version` com timeout de 15 s e exige uma versão
   reconhecível (`installation.rs:340-348`). O wizard mostra versão e caminho
   (`startup.rs:417-431`).
3. **Escolha e nova tentativa.** "Escolher executável existente" e "Já instalei,
   verificar" refazem a descoberta, com mensagem específica para PATH
   desatualizado (`startup.rs:156-165`, `198-203`, `445-459`).
4. **Instalação privada via npm.** Só aparece quando o Node atende
   `>= 22.19.0` e o npm responde (`startup.rs:176-196`, `487-492`). Roda
   `npm install -g --ignore-scripts --no-audit --no-fund --prefix
   <%LOCALAPPDATA%\dish\pi> @earendil-works/pi-coding-agent` e valida o
   `pi.cmd` resultante (`installation.rs:361-391`; diretório em
   `installation.rs:31-38`; prefixo `dish/pi` em `36-38`).
5. **Persistência.** Grava `pi_executable` em `%APPDATA%\dish\state.json`
   (`installation.rs:18-22`; `preferences.rs:54-63`). Numa execução seguinte,
   se já existe preferência salva e o Pi continua válido, o wizard nem aparece
   (`startup.rs:150-161`). Sem preferência, ele mostra o passo "Pronto"
   (`startup.rs:162`).

Em resumo: **descobrir, validar, escolher, instalar via npm e lembrar** já
funcionam no Windows. O wizard não instala Node, não altera o `PATH`, não
compara versões para sugerir atualização e não abre o instalador oficial no
Windows.

## 1.3 O que o NSIS faz

- Instalação **por usuário**, sem administrador, em
  `%LOCALAPPDATA%\Programs\Dish` (`nsi:5`, `37-39`).
- Páginas: boas-vindas, licença, verificação do Pi, componentes, diretório,
  instalação e conclusão (`nsi:55-61`).
- Detecta o Pi **apenas** com `cmd /C where pi` (`nsi:75-84`). Se o comando
  falha, mostra a opção — desmarcada — de baixar e revisar o instalador
  oficial (`nsi:95-101`).
- Copia o app, licença, avisos, pacote de licenças, `BUILD.json` e o helper
  `install-pi-windows.ps1` para `$INSTDIR` (`nsi:159-169`).
- Cria atalhos e chaves de desinstalação; o desinstalador **não** apaga
  preferências, sessões nem a instalação do Pi (`nsi:171-190`, `203-222`).
- Se a opção do Pi foi marcada, abre o helper em outra janela e desabilita
  "Executar o Dish agora" (`nsi:132-154`).
- `grep -in 'node\|npm' scripts/windows-installer.nsi` não retorna nada:
  **não há checagem nem instalação de Node/npm no NSIS**.

O pacote é construído por `scripts/package-windows.py`: monta o payload, gera o
bundle de licenças, roda `makensis` e grava o `.sha256`
(`package-windows.py:51-72`, `92-116`). O rótulo do release vem do `ci.yml`
(`package-windows.py:31-40`). A assinatura é opcional e o build continua válido
sem os secrets (`sign-windows.ps1:3-6`; `windows.yml:50-55`, `98-103`). O
publish só anexa ao Release quando a tag casa com o nome do instalador
(`windows.yml:121-161`).

## 1.4 Sobreposição e divergências

**D1 — Duas definições de "Pi instalado".** O NSIS só aceita `where pi`
(`nsi:77`); o app também procura `%USERPROFILE%\.pi\agent\bin\pi.cmd`,
`%APPDATA%\npm\pi.cmd`, `%LOCALAPPDATA%\Programs\pi\pi.cmd` e
`%LOCALAPPDATA%\pi\pi.cmd` (`installation.rs:197-215`). Consequência: quem
instalou o Pi pelo instalador oficial e recusou ou perdeu a atualização de
`PATH` aparece como "não encontrado" no instalador, mas o app o encontra. Duas
mensagens contraditórias para o mesmo computador.

**D2 — Dois caminhos de instalação de Pi, com UX diferente.**
O NSIS oferece "baixar e revisar o instalador oficial" e executa o helper
(`nsi:95-101`, `147-151`). O app oferece (a) instalação npm privada
(`startup.rs:205-245`) e (b) "Copiar comando manual", que copia exatamente o
mesmo helper (`startup.rs:503-505`; `installation.rs:394-397`). O helper é o
mesmo arquivo, mas a porta de entrada e o consentimento são diferentes. O
helper baixa o instalador oficial do Pi em tempo de execução — o NSIS não
embarca Pi nem Node (`install-pi-windows.ps1:8-10`).

**D3 — Botão morto no Windows.** A tela de instalação do wizard diz que "o
instalador oficial abre em um terminal" e oferece "Aceitar e abrir instalador
oficial" (`startup.rs:473-475`). No Windows,
`installation::launch_official_installer()` retorna erro imediatamente:
"Neste sistema, copie o comando oficial e execute-o em um terminal."
(`installation.rs:284-286`). O botão existe, mas não abre nada; o usuário só
tem o npm (se tiver Node) ou o comando manual.

**D4 — O Dish não tem passo de instalar Node.** O NSIS não checa Node
(nenhuma referência no arquivo); o wizard só verifica Node/npm
(`installation.rs:361-372`; `startup.rs:176-196`). Quem instala Node é o
instalador oficial do Pi, que pergunta antes, baixa o Node 22.x para
`%LOCALAPPDATA%\pi-node\current` e adiciona ao `PATH`
(`pi.dev/install.ps1`, linhas 600-640, 639, 649). Ou seja: o texto do wizard
promete uma dependência que ele não instala e, no Windows, não consegue abrir
quem instala.

**D5 — Duas versões.** O binário usa `CARGO_PKG_VERSION`, hoje `0.1.0`
(`Cargo.toml:3`; `settings.rs:1226`), enquanto o instalador e o release usam
`0.1.0-alpha.5` (`ci.yml:103`; `package-windows.py:31-40`; `nsi:175`). Não há
`--version` no Dish: um argumento inicial com `-` é repassado ao Pi
(`main.rs:198-202`; `update-design.md:25-37`). Comparar versões antes de
embutir o rótulo do release daria o resultado errado (`update-design.md:32-37`).

**D6 — Confiança e assinatura.** A assinatura é opt-in; sem os dois secrets o
build sai sem Authenticode (`sign-windows.ps1:3-6`). O relatório do #102
registra que o alpha.5 publicado tinha Security Directory vazio
(`docs/windows-false-positive.md:17-18`) e que o helper foi desenhado para
revisão explícita (`30-45`). Isso não impede o fluxo, mas pode afetar
SmartScreen e a confiança de quem baixa.

## 1.5 Divisão de responsabilidade recomendada

| Responsabilidade | Dono | Regra |
| --- | --- | --- |
| Colocar `dish.exe`, atalhos, desinstalador, licenças, `BUILD.json`, assinatura/checksum | NSIS | Só empacotamento; sem rede, sem instalar Pi/Node |
| Detectar, validar, escolher e lembrar o Pi | App | Fonte única; o NSIS não deve repetir `where pi` |
| Instalar o Pi | App | Um único caminho explícito (npm privado ou instalador oficial), com consentimento e resultado visível |
| Instalar Node | App (novo) ou instalador oficial | Não colocar no NSIS; qualquer download precisa de versão fixa, hash e consentimento |
| Atualizar o Dish | App (#57) | Design existe; implementação não |
| Atualizar o Pi | Pi (`pi update`) ou npm, conforme o tipo de instalação | Não prometer antes de testar cada tipo |
| Desinstalar | NSIS | Não tocar em preferências, sessões ou Pi (`nsi:221-222`) |

O ponto mais importante: **o instalador não deve ter uma segunda opinião sobre
o estado do Pi**. Ele pode mostrar uma nota informativa, mas a decisão e a
remediação pertencem ao app, que continua rodando depois e tem a busca
ampliada.

## 1.6 Quanto do fluxo cabe hoje

| Etapa | Estado | Onde |
| --- | --- | --- |
| Instalar o app, atalhos, desinstalador, licenças | Pronto | NSIS (`nsi:159-190`, `203-222`) |
| Detectar Pi no `PATH` e em locais conhecidos | Pronto no app; duplicado no NSIS | `installation.rs:193-228`; `nsi:77` |
| Validar `pi --version` | Pronto | `installation.rs:340-348` |
| Escolher executável e repetir verificação | Pronto | `startup.rs:198-203`, `445-459` |
| Instalar Pi via npm, sem terminal e sem admin | Pronto, mas condicionado a Node `>= 22.19` | `startup.rs:205-245`; `installation.rs:361-391` |
| Instalar Pi pelo instalador oficial, em GUI | Não funciona no Windows | `installation.rs:284-286`; `startup.rs:473-475` |
| Instalar Node | Não existe no Dish | Nenhuma referência no NSIS; só checagem no app |
| Lembrar a escolha | Pronto | `installation.rs:18-22`; `startup.rs:150-161` |
| Atualizar o Dish | Só desenho | `docs/update-design.md:3-5` |
| Atualizar o Pi | Não existe no Dish | `pi update` existe no Pi, não é usado aqui |

**Conclusão de viabilidade:** o fluxo cabe no app, exceto a aquisição do Node.
As peças de GUI, consentimento, validação e persistência já existem; o que falta
é uma decisão de produto sobre Node e sobre o instalador oficial. Colocar isso
no NSIS aumentaria a superfície (rede no instalador, elevação para MSI,
duplicação de confiança) sem resolver o caso de quem instala o app antes do Pi.

## 1.7 Versão e atualização

- O Dish não checa atualização: não há cliente HTTP nas dependências
  (`Cargo.toml:8-18`) e o desenho do #57 é explícito sobre não haver
  implementação (`docs/update-design.md:3-5`).
- O "Sobre" mostra `CARGO_PKG_VERSION` (`settings.rs:1226`), que é `0.1.0`, não
  o rótulo `0.1.0-alpha.5` do release (`ci.yml:103`).
- O instalador exibe `VERSION` no `DisplayVersion` (`nsi:175`), recebido do
  empacotador (`package-windows.py:101-109`). Ou seja, instalador e app podem
  mostrar versões diferentes do mesmo release.
- O caminho para corrigir está no #57: embutir o rótulo do release e expor
  `--version` antes de qualquer comparação (`update-design.md:134-146`).

# Parte 2 — Survey de front-ends/clientes de agentes de código

## 2.1 Método e limites

Consultei a documentação oficial de cada produto em 2026-10-27 (lista de URLs
no fim). **Não executei nenhum instalador** e não inspecionei binários: as
afirmações abaixo descrevem o que a documentação consultada declara. Onde a
página não documenta um comportamento, escrevo "não documentado na fonte
consultada" — isso não prova que o recurso não exista.

## 2.2 Comparativo

| Cliente | Como distribui | Onboarding de dependências | Checa versão / atualiza | Fonte |
| --- | --- | --- | --- | --- |
| **Claude Code** (Anthropic): app desktop + CLI | App macOS/Windows; CLI por instalador nativo, npm, Homebrew, WinGet, apt/dnf/apk | App: instalar, entrar e abrir a aba Code; requer assinatura Pro/Max/Team/Enterprise. CLI: instalador nativo recomendado; requisitos não listam Node; npm é alternativa | `claude --version`, `claude doctor`; instalação nativa atualiza em segundo plano; `claude update`; Homebrew/WinGet exigem upgrade manual | [1], [2] |
| **Codex / ChatGPT desktop** (OpenAI) | App ChatGPT macOS/Windows (Linux com guia próprio); CLI com instalador standalone (sh/ps1), npm e Homebrew | App: instalar, entrar, escolher pasta. CLI: instalador standalone; npm/brew como alternativas; a página do CLI não lista Node como pré-requisito | CLI documenta comandos de instalação e atualização; `brew upgrade --cask codex`; atualização do app desktop não documentada na página | [3], [4] |
| **Cursor** | `.dmg`, `.exe`, AppImage, repositórios apt/yum | Abrir, entrar, escolher pasta; nenhum passo de Node documentado | Pacotes apt/yum são descritos como tendo "automatic updates"; atualização do app em si não documentada na página de instalação | [5] |
| **Devin Desktop** (docs do Windsurf redirecionam para cá) | Instaladores Mac/Windows/Linux, tarball, repositórios deb/rpm | Instalar, passar pelo onboarding, importar configurações do VS Code/Cursor; nenhum passo de Node documentado | O tarball "does not update itself"; usar o gerenciador de pacotes; atualização do app nativo não documentada | [6] |
| **Zed** | macOS por download/Homebrew; Windows por download; Linux por script de instalação | Instalar o editor; agentes externos vêm do ACP Registry; Pi aparece como agente ACP e a autenticação/modelos ficam no Pi | Auto-update por padrão: baixa em segundo plano e aplica ao reiniciar; pode ser desligado | [7] |
| **Cline Desktop** | Instalador macOS/Windows (Windows em beta) | Instalar, escolher provedor/modelo; o app é "bring your own model"; nenhum passo de Node documentado | Não documentado na página de instalação consultada | [8] |
| **opencode** | Script de instalação, npm/bun/pnpm/yarn, Homebrew, pacman, Chocolatey/Scoop/npm/mise/Docker no Windows, releases | Instalar; Node só entra no caminho npm | `opencode upgrade` atualiza para a última versão ou uma versão específica | [9] |
| **Pi** (o agente que o Dish usa) | `install.sh`/`install.ps1`, npm, Nix | No Windows, o instalador oficial checa Node `>= 22.19`, oferece baixar Node standalone para `%LOCALAPPDATA%\pi-node\current`, oferece Git for Windows via winget e instala o Pi de forma gerenciada | `pi update`; o instalador fixa dependências; Nix usa `nix profile upgrade` e `pi update` não atualiza instalação Nix | [10] |

## 2.3 Padrões observados

1. **App desktop tende a embutir o runtime.** Cursor, Zed, Cline Desktop,
   Devin Desktop e os apps Claude/ChatGPT não documentam exigir Node do
   usuário; a instalação é o app e, no máximo, login. Isso é possível porque o
   agente vem junto ou é serviço.
2. **Onboarding de dependência é coisa de instalador de terminal.** Pi,
   Claude Code, Codex e opencode documentam instaladores/CLIs com comandos
   explícitos; o Pi vai além e oferece instalar Node e Git no próprio
   instalador, com perguntas no terminal.
3. **Atualização é explícita ou automática, mas separada.** CLIs expõem
   `update`/`upgrade` e, às vezes, auto-update em segundo plano; apps GUI
   atualizam o app inteiro. Nenhum dos comparados documenta "o app GUI detecta
   e instala Node".
4. **Hospedar agente externo delega o onboarding.** Zed instala agentes
   externos pelo ACP Registry e deixa autenticação/configuração no agente;
   não há detecção de Node na página consultada. O Dish está nessa categoria,
   mas com uma dependência adicional específica do Pi: o Node para o caminho
   npm.
5. **O instalador oficial do Pi já cobre as dependências do #13, mas não em GUI.**
   Ele pergunta antes de instalar Node (`pi.dev/install.ps1`, linhas 643-657),
   instala Node standalone com checksum (600-640), oferece Git via winget
   (759-790) e no fim pergunta se adiciona o binário ao `PATH` (1966-1972).
   É interativo e pressupõe terminal; não é um componente embutível sem
   reimplementação.

## 2.4 O que não consegui verificar

- Não rodei nenhum dos instaladores, nem o NSIS, nem o `install.ps1` do Pi.
  Comportamento em máquina real (PATH, SmartScreen, antivírus, permissões) não
  foi testado.
- A documentação do Cursor, Cline, do app ChatGPT e do Devin Desktop
  consultada não descreve atualização do app; não afirmo que não exista.
- Não verifiquei se `pi update` atualiza uma instalação feita pelo Dish em
  `%LOCALAPPDATA%\dish\pi` com `npm --prefix`; o instalador do Pi usa um
  "managed install" com marcador e lock próprios
  (`pi.dev/install.ps1`, linhas 8-9, 1428, 1464-1486), que é diferente do
  caminho npm do Dish (`installation.rs:374-391`).
- Não verifiquei o estado do repositório em relação a secrets de assinatura:
  o código aceita build sem assinatura (`sign-windows.ps1:3-6`), mas não sei
  se o repositório tem os secrets configurados.
- O survey é documental e datado; preços, planos e nomes de produtos mudam.

# Recomendação e próximos passos

## Decisão que o #13 precisa tomar

Há duas direções coerentes; misturá-las é o que produz o botão morto atual.

**A. GUI de verdade (sem terminal).** O app passa a ser dono também do Node:
baixar uma versão fixa do Node para Windows, verificar SHA-256 contra o
`SHASUMS256.txt` oficial, extrair em `%LOCALAPPDATA%\dish\node` (ou similar) e
usar esse Node no `npm install` que já existe. Vantagens: cumpre o "next-next"
do #13, não exige admin, não depende de terminal. Custos: novo código de
download/verificação, política de versão do Node, licença do Node no bundle de
terceiros e superfície de segurança. O helper atual já mostra o modelo de
consentimento a seguir (`install-pi-windows.ps1:8-24`).

**B. Handoff honesto para o instalador oficial.** Manter o instalador oficial
como caminho principal e assumir que ele é interativo: no Windows, abrir o
helper em um PowerShell visível (como o NSIS faz) ou exibir o comando copiável,
e remover o botão que promete abrir algo que não abre. Vantagens: não
reimplementa Node/Pi, segue o modelo do Linux. Custos: não cumpre "GUI, sem
terminal" do #13; o usuário vê PowerShell, Notepad e a pergunta `INSTALAR`.

Para o curto prazo, **B é a correção de consistência** (o botão deve fazer o
que diz); **A é o escopo real do #13** se a exigência de GUI for mantida. Em
qualquer caso, o NSIS não deve instalar Pi/Node.

## Ordem sugerida

1. **Corrigir a mentira da UI no Windows**: ou o botão abre o helper, ou some e
   dá lugar ao comando copiável, com o texto do que vai acontecer
   (`startup.rs:465-509`; `installation.rs:284-286`).
2. **Tirar a detecção de Pi do NSIS** (ou rebaixá-la a nota informativa). A
   fonte única é `installation.rs:193-228`; o NSIS só deve instalar o app e
   sempre permitir abrir o Dish.
3. **Uma versão só**: embutir o rótulo do release e expor `--version`
   (`update-design.md:134-146`), antes de qualquer banner de atualização.
4. **Definir o dono da atualização do Pi**: testar `pi update` em (a)
   instalação gerenciada do instalador oficial, (b) prefixo npm privado do
   Dish, (c) npm global. Só oferecer o botão depois de saber o que cada caso
   faz; para o prefixo privado, o caminho a testar é npm, não `pi update`.
5. **Se A for escolhido**, especificar o download de Node com versão fixa,
   hash, destino por usuário, consentimento e entrada de licença; tratar como
   componente de terceiro, não como detalhe interno.

## O que não fazer

- NSIS baixando Node/Pi em silêncio, ou com `ExecutionPolicy Bypass`/`iex`:
  o #102 já documentou o risco e o helper atual evita isso
  (`docs/windows-false-positive.md:30-45`).
- Duas telas com regras diferentes para "Pi instalado".
- Comparar `0.1.0` com `0.1.0-alpha.5` como se fossem a mesma escala.
- Prometer "atualizar o Pi" sem distinguir instalação gerenciada, npm global e
  prefixo privado do Dish.

# Evidências verificadas no repositório

Comandos usados na conferência: `git grep -n`, `nl -ba | sed -n` e leitura dos
arquivos citados. Todos os caminhos existem no worktree em `51a753e`.

- `scripts/windows-installer.nsi:5,37-39` — instalação por usuário, sem admin.
- `scripts/windows-installer.nsi:55-61` — ordem das páginas.
- `scripts/windows-installer.nsi:75-101` — `where pi`, opção desmarcada.
- `scripts/windows-installer.nsi:132-154` — fim do instalador e handoff.
- `scripts/windows-installer.nsi:159-190,203-222` — payload, atalhos, uninstall.
- `scripts/install-pi-windows.ps1:8-24` — download, revisão, `INSTALAR`, execução.
- `scripts/sign-windows.ps1:3-6` — assinatura opt-in.
- `src/installation.rs:18-22,31-38,48-55` — persistência, diretório e descoberta.
- `src/installation.rs:193-228` — busca fora do `PATH`.
- `src/installation.rs:284-286` — instalador oficial recusado fora do Linux.
- `src/installation.rs:340-348,361-391` — validação, Node/npm, npm install.
- `src/startup.rs:150-161` — pula o wizard para quem já tem escolha.
- `src/startup.rs:176-245` — checagem de Node e instalação npm.
- `src/startup.rs:465-509` — tela de instalação no Windows.
- `src/settings.rs:1226` — "Sobre" usa `CARGO_PKG_VERSION`.
- `Cargo.toml:3,8-18` — versão `0.1.0`, sem cliente HTTP.
- `src/main.rs:198-202` — flags iniciais vão para o Pi.
- `scripts/package-windows.py:31-40,92-116` — rótulo e empacotamento.
- `.github/workflows/windows.yml:50-55,98-103,121-161` — assinatura e publish.
- `docs/update-design.md:3-5,25-37,134-146` — design, versão e `--version`.
- `docs/windows-false-positive.md:17-18,30-45,58-80` — #102, helper e assinatura.

# Fontes externas

Consultadas em 2026-10-27. A documentação muda; os números de linha de scripts
baixados valem para a versão obtida nesta data.

1. Claude Code — setup: <https://code.claude.com/docs/en/setup>
2. Claude Code — desktop: <https://code.claude.com/docs/en/desktop-quickstart>
3. Codex CLI: <https://developers.openai.com/codex/cli.md>
4. Codex app (ChatGPT desktop): <https://developers.openai.com/codex/app.md>
5. Cursor — instalação: <https://cursor.com/docs/get-started/installation>
   (redireciona para <https://cursor.com/docs/get-started/quickstart>)
6. Devin Desktop — getting started:
   <https://docs.devin.ai/desktop/getting-started> (alcançado a partir de
   <https://docs.windsurf.com/windsurf/getting-started>, que redireciona)
7. Zed — instalação: <https://zed.dev/docs/installation>; atualização:
   <https://zed.dev/docs/update.html> (redireciona para
   <https://zed.dev/docs/update>); agentes externos:
   <https://zed.dev/docs/ai/external-agents>
8. Cline — instalação:
   <https://docs.cline.bot/getting-started/installing-cline>; desktop:
   <https://docs.cline.bot/usage/cline-desktop>
9. opencode: <https://opencode.ai/docs/>; CLI/upgrade:
   <https://opencode.ai/docs/cli/>
10. Pi — quickstart: <https://pi.dev/docs/latest/quickstart>; CLI:
    <https://pi.dev/docs/latest/cli>; instalador Windows:
    <https://pi.dev/install.ps1>; Windows setup:
    <https://pi.dev/docs/latest/windows>
