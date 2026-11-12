# Instalador do Windows (NSIS)

O instalador fica em `scripts/windows-installer.nsi`. `scripts/package-windows.py`
monta o payload (o `dish.exe`, as licenças e o `BUILD.json`) e chama o
`makensis` no job `windows` do CI; o artefato é
`dist/dish-<versão>-windows-x86_64-setup.exe`. A instalação é por usuário
(`RequestExecutionLevel user`), em `%LOCALAPPDATA%`, sem UAC.

## Páginas

1. Boas-vindas
2. Licença
3. Componentes (Dish, menu Iniciar, atalho na área de trabalho)
4. Pasta de instalação
5. **Pi e Node.js** — pulada inteira quando os dois já estão presentes
6. Instalando — barra de progresso e o registro com cada passo
7. Conclusão — "Abrir o Dish agora" já marcado

## Detecção

O Node.js é procurado primeiro no `PATH` (`where node.exe`) e depois em
`%ProgramFiles%\nodejs\node.exe`, `%LOCALAPPDATA%\pi-node\current\node.exe` e
`%LOCALAPPDATA%\dish\node\node.exe`. Só vale o candidato cujo `--version`
responde com sucesso e cuja versão seja **22.19.0 ou superior**; um Node antigo
aparece na página como aviso e a busca continua. Quando o Node é achado fora do
`PATH`, a página diz isso explicitamente.

O Pi segue a mesma ideia: `where pi` e, depois, os caminhos que o app já usa em
`src/installation.rs` (`.local\bin`, `.npm-global\bin`, `.pi\agent\bin`,
`%APPDATA%\npm`, `%LOCALAPPDATA%\Programs\pi`, `%LOCALAPPDATA%\pi`) mais o
prefixo privado `%LOCALAPPDATA%\dish\pi`. Cada candidato precisa responder a
`--version`; o `.cmd` é executado via `cmd /C` com as aspas externas exigidas
por caminhos com espaço.

## O que é instalado e onde

- **Node.js 24.22.2**, versão fixa (não "latest"), baixado de
  `https://nodejs.org/dist/v24.22.2/` e conferido pelo SHA-256 publicado em
  `SHASUMS256.txt`. Vai para `%LOCALAPPDATA%\dish\node` (`node.exe` e
  `npm.cmd`) e o diretório é acrescentado ao `PATH` **do usuário** de forma
  idempotente. A extração acontece em `node.new` e só então é renomeada para o
  destino, para uma falha não deixar uma instalação pela metade.
- **Pi**, via `npm install -g --ignore-scripts --no-audit --no-fund --prefix`,
  os mesmos argumentos de `src/installation.rs`, em `%LOCALAPPDATA%\dish\pi`
  (`pi.cmd` na raiz do prefixo). O `PATH` não muda: o Dish procura nesse
  prefixo privado.

O instalador não usa PowerShell. O download usa a API do Windows
(`urlmon::URLDownloadToFileW`, pelo plug-in `System` que acompanha o NSIS), a
conferência usa o `certutil.exe` do próprio Windows e a extração usa o `tar.exe`
(bsdtar) do Windows 10 1803+. Nada é baixado no build e nenhum utilitário é
instalado.

O `PATH` do usuário é lido com `ReadRegStr` (que devolve o `REG_EXPAND_SZ` como
está, sem expandir `%VAR%`) e gravado com `WriteRegExpandStr`; depois da escrita
o instalador envia um `WM_SETTINGCHANGE` para o Explorer recarregar o ambiente.
Como as strings do NSIS têm 1024 caracteres, se o `PATH` não couber junto com o
novo diretório o instalador **não escreve nada** e mostra uma mensagem para
acrescentar `%LOCALAPPDATA%\dish\node` manualmente: ele nunca grava um `PATH`
truncado. O `npm.cmd` recebe o diretório do Node pelo próprio `cmd`
(`set "PATH=...;%PATH%"`), sem alterar o ambiente do processo do instalador.

## Consentimento

Nada é baixado sem escolha explícita: as caixas de Node e de Pi começam
**desmarcadas**. Um `/S` (instalação silenciosa) não executa páginas customizadas,
logo as variáveis ficam vazias e a seção de dependências não instala nada. O
download usa o proxy e o TLS do sistema; não há shell, script externo, `iex`
nem arquivo `.ps1` no fluxo. A saída do npm não vai para o
registro porque pode conter credencial de registry privado; ela fica num arquivo
temporário e apenas as linhas de código de erro são mostradas.

## Falhas

O Dish é instalado antes das dependências (`SEC_MAIN` vem primeiro), então pular
ou falhar a etapa de Node/Pi nunca deixa o aplicativo pela metade. Em caso de
erro, a caixa oferece **Tentar de novo** ou **Cancelar** (pula a etapa e mostra o
erro real no registro). Se o Node falhar, o passo do Pi também é cancelado. Na
conclusão, se o Pi não tiver sido instalado e a pessoa tinha marcado a instalação,
uma linha avisa que o Dish abrirá o próprio assistente de primeira execução — que
continua sendo o caminho de recuperação.

## Desinstalação

O desinstalador remove só os arquivos do Dish. O Node em
`%LOCALAPPDATA%\dish\node`, a entrada correspondente no `PATH` e o Pi em
`%LOCALAPPDATA%\dish\pi` permanecem, como as preferências e as sessões: o Dish
não pode apagar conversas nem mexer na instalação do Pi.

## Atualizar o Node fixado

Troque `NODE_VERSION` e `NODE_SHA256` em `scripts/windows-installer.nsi` pela
versão desejada e pelo hash da linha `node-v<versão>-win-x64.zip` em
`https://nodejs.org/dist/v<versão>/SHASUMS256.txt`. O download e a conferência
usam esses dois defines; não há consulta a "latest".

## Limites

Esta branch não foi testada numa máquina Windows real. A validação automática é
o build do `makensis` no CI; o teste manual recomendado é: VM nova sem Node,
Node antigo (abaixo de 22.19.0), Pi instalado fora do `PATH`, instalador offline
(erro de download e "Tentar de novo"), `PATH` do usuário acima de 1024
caracteres (mensagem clara e `PATH` intacto) e reexecução para conferir que o
`PATH` e o prefixo do Pi não duplicam.

O fluxo depende de três componentes do Windows 10 1803 ou superior: `urlmon`
(WinINet, para o download), `certutil.exe` (SHA-256) e `tar.exe` (zip). Se algum
faltar, a etapa falha com mensagem e o app continua instalado; nada é baixado
para substituí-los.

O diretório do Node é acrescentado ao `PATH` **do usuário**, e o Windows coloca o
`PATH` da máquina na frente. Se houver um Node antigo (abaixo de 22.19.0) no
`PATH` da máquina, o `pi.cmd` o encontra na hora de rodar, mesmo com o Node novo
instalado; nesse caso o assistente de primeira execução do Dish informa que o Pi
não responde. A correção é atualizar ou remover esse Node antigo: o instalador
nunca mexe no `PATH` da máquina (ele não roda elevado).

O `scripts/install-pi-windows.ps1` continua no pacote e continua sendo usado pelo
botão de "copiar o comando manual" do app; o instalador não o abre mais.
