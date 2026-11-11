; Instalador do Dish para Windows (NSIS).
;
; Escolha por um EXE/NSIS em vez de MSI: o fluxo pedido na issue #13 é um
; assistente (avançar/avançar/concluir) com detecção do Pi e do Node.js, e o
; NSIS faz isso sem exigir administrador — a instalação é por usuário, em
; %LOCALAPPDATA%.
;
; O Dish continua sendo só o cliente. A página "Pi e Node.js" mostra o que já
; existe e, com consentimento explícito, instala o Node.js fixado (conferido
; por SHA-256) em %LOCALAPPDATA%\dish\node e o Pi via npm em
; %LOCALAPPDATA%\dish\pi durante a página "Instalando" — sem terminal para a
; pessoa. Se o download falhar ou a pessoa pular, o assistente de primeira
; execução do Dish cobre o mesmo passo.
;
; Compilar:  makensis /DVERSION=0.1.0-alpha.3 /DVERSIONNUM=0.1.0.3 /DSRCDIR=..\.. scripts\windows-installer.nsi

Unicode true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!include "WinMessages.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef VERSIONNUM
  !define VERSIONNUM "0.0.0.0"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\.."
!endif

!define APP_NAME "Dish"
!define APP_EXE "dish.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Dish"

; Versão fixada, não "latest": o instalador baixa o mesmo bytes sempre.
!define NODE_VERSION "24.22.2"
!define NODE_MIN "22.19.0"
; SHA-256 de node-v24.22.2-win-x64.zip, conferido em https://nodejs.org/dist/v24.22.2/SHASUMS256.txt
!define NODE_SHA256 "ca1bff5d8ead13c9f2ea404a176db46bd430704d5b5113734049fa365e7799e2"
!define NODE_ZIP "node-v${NODE_VERSION}-win-x64"
!define NODE_URL "https://nodejs.org/dist/v${NODE_VERSION}/${NODE_ZIP}.zip"
; 64 dígitos hexadecimais (32 bytes) de um SHA-256.
!define SHA256_HEX_LEN 64
!define PI_PACKAGE "@earendil-works/pi-coding-agent"

Name "${APP_NAME} ${VERSION}"
OutFile "${SRCDIR}\dist\dish-${VERSION}-windows-x86_64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Dish"
InstallDirRegKey HKCU "Software\Dish" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSIONNUM}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "Instalador do Dish"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Apache-2.0"

!define MUI_ICON "${SRCDIR}\assets\dish-icon.ico"
!define MUI_UNICON "${SRCDIR}\assets\dish-icon.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${SRCDIR}\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
Page custom DepsPage DepsLeave
!insertmacro MUI_PAGE_INSTFILES
Page custom FinishPage FinishLeave

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "English"

Var NodeOk
Var NodeExe
Var NodeVer
Var NodeNote
Var PiOk
Var PiExe
Var PiVer
Var PiNote
Var WantNode
Var WantPi
Var NodeCheckbox
Var PiCheckbox
Var DishCheckbox
Var RunDish
Var PathError

; --------------------------------------------------------------- detecção

; Primeira linha de uma saída (caminho/versão), sem CR/LF e sem espaços nas
; pontas: `where` e os .cmd do npm escrevem mais de uma linha.
Function FirstLine
  Exch $R0
  Push $R1
  Push $R2
  Push $R3
  StrCpy $R1 ""
  StrCpy $R2 0
  ${Do}
    StrCpy $R3 $R0 1 $R2
    ${If} $R3 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R3 == "$\r"
    ${OrIf} $R3 == "$\n"
      ${ExitDo}
    ${EndIf}
    StrCpy $R1 "$R1$R3"
    IntOp $R2 $R2 + 1
  ${Loop}
  ${Do}
    StrCpy $R3 $R1 1
    ${If} $R3 != " "
      ${ExitDo}
    ${EndIf}
    StrCpy $R1 $R1 "" 1
  ${Loop}
  ${Do}
    StrCpy $R3 $R1 "" -1
    ${If} $R3 != " "
      ${ExitDo}
    ${EndIf}
    StrCpy $R1 $R1 -1
  ${Loop}
  StrCpy $R0 $R1
  Pop $R3
  Pop $R2
  Pop $R1
  Exch $R0
FunctionEnd

; Caminho no topo da pilha. Só muda as variáveis de detecção se o executável
; responder a --version; um Node antigo vira nota e a busca continua.
Function TryNode
  Exch $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  nsExec::ExecToStack '"$R0" --version'
  Pop $R1
  Pop $R2
  ${If} $R1 == "0"
    Push $R2
    Call FirstLine
    Pop $R3
    ${If} $R3 != ""
      StrCpy $R4 $R3 1
      ${If} $R4 == "v"
        StrCpy $R3 $R3 "" 1
      ${EndIf}
      ${VersionCompare} "$R3" "${NODE_MIN}" $R1
      ${If} $R1 == "0"
      ${OrIf} $R1 == "1"
        StrCpy $NodeOk "1"
        StrCpy $NodeExe "$R0"
        StrCpy $NodeVer "$R3"
        StrCpy $NodeNote "Node.js $R3: $R0"
      ${Else}
        StrCpy $NodeNote "Node.js $R3 encontrado em $R0, mas o Pi exige ${NODE_MIN} ou superior."
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
FunctionEnd

; O cmd precisa das aspas duplas externas para aceitar um caminho com espaços.
Function TryPi
  Exch $R0
  Push $R1
  Push $R2
  Push $R3
  nsExec::ExecToStack 'cmd /C ""$R0" --version"'
  Pop $R1
  Pop $R2
  ${If} $R1 == "0"
    Push $R2
    Call FirstLine
    Pop $R3
    ${If} $R3 != ""
      StrCpy $PiOk "1"
      StrCpy $PiExe "$R0"
      StrCpy $PiVer "$R3"
      StrCpy $PiNote "Pi $R3: $R0"
    ${EndIf}
  ${EndIf}
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
FunctionEnd

!macro TryNodeCandidate CANDIDATE
  ${If} $NodeOk != "1"
    ${If} ${FileExists} `${CANDIDATE}`
      Push `${CANDIDATE}`
      Call TryNode
    ${EndIf}
  ${EndIf}
!macroend

!macro TryPiCandidate CANDIDATE
  ${If} $PiOk != "1"
    ${If} ${FileExists} `${CANDIDATE}`
      Push `${CANDIDATE}`
      Call TryPi
    ${EndIf}
  ${EndIf}
!macroend

Function DetectDeps
  StrCpy $NodeOk "0"
  StrCpy $NodeExe ""
  StrCpy $NodeVer ""
  StrCpy $NodeNote ""
  StrCpy $PiOk "0"
  StrCpy $PiExe ""
  StrCpy $PiVer ""
  StrCpy $PiNote ""

  ; `where` usa o PATH deste processo. Depois de instalar, o PATH do
  ; instalador continua velho, então os caminhos fixos abaixo são a garantia.
  nsExec::ExecToStack 'cmd /C where node.exe'
  Pop $0
  Pop $1
  ${If} $0 == "0"
    Push $1
    Call FirstLine
    Pop $0
    ${If} $0 != ""
      Push $0
      Call TryNode
    ${EndIf}
  ${EndIf}

  ${If} $NodeOk != "1"
    !insertmacro TryNodeCandidate "$PROGRAMFILES64\nodejs\node.exe"
    !insertmacro TryNodeCandidate "$LOCALAPPDATA\pi-node\current\node.exe"
    !insertmacro TryNodeCandidate "$LOCALAPPDATA\dish\node\node.exe"
    ${If} $NodeOk == "1"
      StrCpy $NodeNote "$NodeNote (fora do PATH)"
    ${EndIf}
  ${EndIf}

  ${If} $NodeOk != "1"
  ${AndIf} $NodeNote == ""
    StrCpy $NodeNote "Node.js não encontrado."
  ${EndIf}

  nsExec::ExecToStack 'cmd /C where pi'
  Pop $0
  Pop $1
  ${If} $0 == "0"
    Push $1
    Call FirstLine
    Pop $0
    ${If} $0 != ""
      Push $0
      Call TryPi
    ${EndIf}
  ${EndIf}

  ; Mesma busca do app (src/installation.rs) mais o prefixo privado do Dish.
  ${If} $PiOk != "1"
    !insertmacro TryPiCandidate "$PROFILE\.local\bin\pi.cmd"
    !insertmacro TryPiCandidate "$PROFILE\.local\bin\pi.exe"
    !insertmacro TryPiCandidate "$PROFILE\.npm-global\bin\pi.cmd"
    !insertmacro TryPiCandidate "$PROFILE\.pi\agent\bin\pi.cmd"
    !insertmacro TryPiCandidate "$PROFILE\.pi\agent\bin\pi.exe"
    !insertmacro TryPiCandidate "$APPDATA\npm\pi.cmd"
    !insertmacro TryPiCandidate "$LOCALAPPDATA\Programs\pi\pi.cmd"
    !insertmacro TryPiCandidate "$LOCALAPPDATA\pi\pi.cmd"
    !insertmacro TryPiCandidate "$LOCALAPPDATA\dish\pi\pi.cmd"
    ${If} $PiOk == "1"
      StrCpy $PiNote "$PiNote (fora do PATH; o Dish o encontra mesmo assim)"
    ${EndIf}
  ${EndIf}

  ${If} $PiOk != "1"
    StrCpy $PiNote "Pi não encontrado."
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------- página de dependências

Function DepsPage
  Call DetectDeps

  ; Se não falta nada, a página nem aparece (nada de página vazia).
  ${If} $NodeOk == "1"
  ${AndIf} $PiOk == "1"
    StrCpy $WantNode "0"
    StrCpy $WantPi "0"
    Abort
  ${EndIf}

  StrCpy $NodeCheckbox ""
  StrCpy $PiCheckbox ""

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "Pi e Node.js" "O Dish precisa do Pi, que roda sobre o Node.js."

  ${NSD_CreateLabel} 0 0 100% 30u "Encontrado neste computador:$\r$\n$NodeNote$\r$\n$PiNote"
  Pop $0

  ${If} $NodeOk != "1"
    ; Desmarcado: nada é baixado sem escolha explícita (política da issue #102).
    ${NSD_CreateCheckbox} 0 36u 100% 12u "Instalar o Node.js ${NODE_VERSION} em %LOCALAPPDATA%\dish\node e adicioná-lo ao PATH do usuário"
    Pop $NodeCheckbox
    ${NSD_Uncheck} $NodeCheckbox
    ${NSD_CreateLabel} 0 50u 100% 10u "Baixado de nodejs.org e conferido por SHA-256 antes de extrair."
    Pop $0
  ${EndIf}

  ${If} $PiOk != "1"
    ${NSD_CreateCheckbox} 0 64u 100% 12u "Instalar o Pi (npm ${PI_PACKAGE}) em %LOCALAPPDATA%\dish\pi"
    Pop $PiCheckbox
    ${NSD_Uncheck} $PiCheckbox
  ${EndIf}

  ${NSD_CreateLabel} 0 84u 100% 36u "Nada é instalado sem marcar. Se você pular, ou se o download falhar, o Dish abre o próprio assistente de primeira execução."
  Pop $0

  nsDialogs::Show
FunctionEnd

Function DepsLeave
  StrCpy $WantNode "0"
  StrCpy $WantPi "0"
  ${If} $NodeCheckbox != ""
    ${NSD_GetState} $NodeCheckbox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $WantNode "1"
    ${EndIf}
  ${EndIf}
  ${If} $PiCheckbox != ""
    ${NSD_GetState} $PiCheckbox $0
    ${If} $0 == ${BST_CHECKED}
      StrCpy $WantPi "1"
    ${EndIf}
  ${EndIf}
  ${If} $WantPi == "1"
  ${AndIf} $NodeOk != "1"
  ${AndIf} $WantNode != "1"
    MessageBox MB_OK|MB_ICONEXCLAMATION "O Pi precisa do Node.js ${NODE_MIN} ou superior. Marque também a instalação do Node.js ou desmarque o Pi."
    Abort
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------- página final

Function FinishPage
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 24u "A instalação do Dish terminou."
  Pop $0
  ${NSD_CreateCheckbox} 0 32u 100% 12u "Abrir o Dish agora"
  Pop $DishCheckbox
  ${NSD_Check} $DishCheckbox
  ${If} $PiOk != "1"
    ${If} $WantNode == "1"
    ${OrIf} $WantPi == "1"
      ${NSD_CreateLabel} 0 52u 100% 36u "O Pi não foi instalado; o Dish abrirá o assistente de primeira execução."
      Pop $0
    ${EndIf}
  ${EndIf}
  nsDialogs::Show
FunctionEnd

Function FinishLeave
  ${NSD_GetState} $DishCheckbox $RunDish
  ${If} $RunDish == ${BST_CHECKED}
    ; O Explorer inicia o Dish com o ambiente do usuário já atualizado (o PATH
    ; com o Node.js) e nunca elevado; um filho do instalador herdaria o PATH
    ; antigo do processo.
    Exec '"$WINDIR\explorer.exe" "$INSTDIR\${APP_EXE}"'
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------- seções

Section "Dish" SEC_MAIN
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\dist\windows-payload\${APP_EXE}"
  File "${SRCDIR}\dist\windows-payload\LICENSE"
  File "${SRCDIR}\dist\windows-payload\THIRD_PARTY_NOTICES.md"
  ; O pacote completo de licenças (dependências, nativas e do Rust), igual ao
  ; que o .tar.gz/.deb do Linux carrega — issue #63.
  File /r "${SRCDIR}\dist\windows-payload\licenses"
  File "${SRCDIR}\dist\windows-payload\BUILD.json"
  File "${SRCDIR}\scripts\install-pi-windows.ps1"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Dish" "InstallDir" "$INSTDIR"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Cleison Santos"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Atalho no menu Iniciar" SEC_STARTMENU
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\Desinstalar ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section /o "Atalho na área de trabalho" SEC_DESKTOP
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

; Depois do Dish: se esta etapa falhar ou for pulada, o app já está completo.
; O nome com "-" esconde a seção da página de componentes.
Section "-Dependencias" SEC_DEPS
  ; Recheca para a página final saber o estado real.
  Call DetectDeps
  ${If} $WantNode == "1"
    Call InstallNode
  ${EndIf}
  ${If} $WantPi == "1"
    Call InstallPi
  ${EndIf}
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} "O aplicativo, a licença, o aviso de terceiros e o pacote de licenças das dependências."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_STARTMENU} "Atalhos no menu Iniciar, incluindo o desinstalador."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} "Um atalho na área de trabalho."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; --------------------------------------------------------------- instalação das dependências

Function InstallNode
  ${Do}
    Call InstallNodeOnce
    Pop $0
    ${If} $0 == "ok"
      ${Break}
    ${EndIf}
    DetailPrint "Falha ao instalar o Node.js: $0"
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Não foi possível instalar o Node.js.$\r$\n$\r$\n$0$\r$\n$\r$\nTentar de novo? Em Cancelar, esta etapa é pulada e o Dish abre o assistente de primeira execução." /SD IDCANCEL IDRETRY nodeRetry
    StrCpy $WantPi "0"
    DetailPrint "Instalação do Node.js pulada."
    ${Break}
    nodeRetry:
  ${Loop}
FunctionEnd

; --------------------------------------------------------------- verificação e PATH

; 1 se o texto contém o SHA-256 esperado. Ignora espaços e quebras de linha
; (o certutil separa os bytes com espaço nas versões antigas) e não diferencia
; maiúsculas de minúsculas (StrCmp é case-insensitive).
Function Sha256InText
  Exch $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  StrCpy $R1 ""
  StrCpy $R2 0
  ${Do}
    StrCpy $R3 $R0 1 $R2
    ${If} $R3 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R3 != " "
    ${AndIf} $R3 != "$\r"
    ${AndIf} $R3 != "$\n"
      StrCpy $R1 "$R1$R3"
    ${EndIf}
    IntOp $R2 $R2 + 1
  ${Loop}
  StrCpy $R5 "0"
  StrCpy $R2 0
  ${Do}
    StrCpy $R3 $R1 ${SHA256_HEX_LEN} $R2
    StrLen $R4 $R3
    ${If} $R4 < ${SHA256_HEX_LEN}
      ${ExitDo}
    ${EndIf}
    ${If} $R3 == "${NODE_SHA256}"
      StrCpy $R5 "1"
      ${ExitDo}
    ${EndIf}
    IntOp $R2 $R2 + 1
  ${Loop}
  StrCpy $R0 $R5
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Exch $R0
FunctionEnd

; Acrescenta %LOCALAPPDATA%\dish\node ao PATH do usuário sem shell externo.
; ReadRegStr devolve o REG_EXPAND_SZ como está (sem expandir %VAR%) e
; WriteRegExpandStr mantém o tipo. O limite de ${NSIS_MAX_STRLEN} caracteres
; das strings do NSIS é tratado de frente: se acrescentar o Node passar do
; limite, nada é escrito e a mensagem pede a inclusão manual — nunca trunca.
; Saída: pilha = "ok" ou a mensagem de erro.
Function UpdateUserPath
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R9
  StrCpy $PathError ""

  ; EnumRegValue diz se o valor existe mesmo quando ReadRegStr não consegue
  ; lê-lo (acima do limite): separa "sem PATH" de "PATH ilegível".
  StrCpy $R3 "0"
  StrCpy $R4 0
  ${Do}
    EnumRegValue $R2 HKCU "Environment" $R4
    ${If} ${Errors}
      ${ExitDo}
    ${EndIf}
    ${If} $R2 == "Path"
      StrCpy $R3 "1"
      ${ExitDo}
    ${EndIf}
    IntOp $R4 $R4 + 1
  ${Loop}

  ReadRegStr $R0 HKCU "Environment" "Path"
  ${If} ${Errors}
    ${If} $R3 == "1"
      StrCpy $PathError "o PATH do usuário passa de ${NSIS_MAX_STRLEN} caracteres e não pode ser lido pelo instalador; ele não foi alterado. Acrescente %LOCALAPPDATA%\dish\node ao PATH manualmente."
      Goto pathFinish
    ${EndIf}
    StrCpy $R0 ""
  ${EndIf}

  ; Idempotência: compara entrada por entrada, sem barra final nem aspas.
  StrCpy $R9 "0"
  StrCpy $R1 "$R0;"
  StrCpy $R2 ""
  StrCpy $R4 0
  ${Do}
    StrCpy $R3 $R1 1 $R4
    ${If} $R3 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R3 == ";"
      StrCpy $R3 $R2 "" -1
      ${If} $R3 == "\"
      ${OrIf} $R3 == '"'
        StrCpy $R2 $R2 -1
      ${EndIf}
      StrCpy $R3 $R2 1
      ${If} $R3 == '"'
        StrCpy $R2 $R2 "" 1
      ${EndIf}
      ${If} $R2 == "$LOCALAPPDATA\dish\node"
      ${OrIf} $R2 == "%LOCALAPPDATA%\dish\node"
        StrCpy $R9 "1"
        ${ExitDo}
      ${EndIf}
      StrCpy $R2 ""
    ${Else}
      StrCpy $R2 "$R2$R3"
    ${EndIf}
    IntOp $R4 $R4 + 1
  ${Loop}

  ${If} $R9 == "1"
    DetailPrint "O PATH do usuário já contém o Node.js."
    Goto pathFinish
  ${EndIf}

  ; O PATH novo precisa caber inteiro na string do NSIS (com ';' e terminador).
  StrLen $R1 "$LOCALAPPDATA\dish\node"
  IntOp $R1 $R1 + 1
  IntOp $R2 ${NSIS_MAX_STRLEN} - 1
  IntOp $R2 $R2 - $R1
  StrLen $R3 "$R0"
  ${If} $R3 > $R2
    StrCpy $PathError "o PATH do usuário tem $R3 caracteres e acrescentar o Node.js passaria do limite de ${NSIS_MAX_STRLEN} do instalador; ele não foi alterado. Acrescente %LOCALAPPDATA%\dish\node ao PATH manualmente."
    Goto pathFinish
  ${EndIf}

  ${If} $R0 == ""
    StrCpy $R1 "$LOCALAPPDATA\dish\node"
  ${Else}
    StrCpy $R1 "$R0;$LOCALAPPDATA\dish\node"
  ${EndIf}
  WriteRegExpandStr HKCU "Environment" "Path" "$R1"
  ${If} ${Errors}
    StrCpy $PathError "não foi possível gravar HKCU\Environment\Path"
    Goto pathFinish
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  DetailPrint "Node.js adicionado ao PATH do usuário."

  pathFinish:
  Pop $R9
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
  ${If} $PathError == ""
    Push "ok"
  ${Else}
    Push "$PathError"
  ${EndIf}
FunctionEnd

; Nenhum interpretador de script: o download usa a API do Windows (urlmon,
; pelo plug-in System que acompanha o NSIS), a conferência usa o certutil.exe do
; Windows e a extração usa o tar.exe do Windows 10 1803+. Nada é baixado no build.
Function InstallNodeOnce
  InitPluginsDir
  StrCpy $0 "$PLUGINSDIR\${NODE_ZIP}.zip"
  Delete "$0"

  DetailPrint "Baixando o Node.js ${NODE_VERSION} de nodejs.org..."
  ; URLDownloadToFileW devolve S_OK (0) e usa HTTPS/proxy do sistema. A API não
  ; informa o status HTTP (um 404 viraria um arquivo de erro): o SHA-256 abaixo
  ; é a garantia de conteúdo.
  System::Call 'urlmon::URLDownloadToFileW(p 0, w "${NODE_URL}", w "$0", i 0, p 0) i .r1'
  ${If} $1 != "0"
    Delete "$0"
    Push "download falhou (código $1). Confira a rede ou o proxy."
    Return
  ${EndIf}

  DetailPrint "Conferindo o SHA-256 do arquivo baixado..."
  ; certutil -hashfile é o verificador SHA-256 que acompanha o Windows: nenhum
  ; utilitário é baixado e nenhum interpretador de script é chamado. Saída
  ; ausente, truncada ou com outro hash não casa com ${NODE_SHA256}: para.
  ; $SYSDIR é SysWOW64 num instalador 32-bit; Sysnative cobre o caso em que o
  ; certutil não está lá.
  StrCpy $1 "$SYSDIR\certutil.exe"
  ${IfNot} ${FileExists} "$1"
    StrCpy $1 "$WINDIR\Sysnative\certutil.exe"
  ${EndIf}
  ${IfNot} ${FileExists} "$1"
    Delete "$0"
    Push "certutil.exe não encontrado: não é possível conferir o SHA-256 do Node.js"
    Return
  ${EndIf}
  nsExec::ExecToStack '"$1" -hashfile "$0" SHA256'
  Pop $1
  Pop $2
  ${If} $1 != "0"
    Delete "$0"
    Push "não foi possível conferir o SHA-256 (certutil retornou $1)"
    Return
  ${EndIf}
  Push $2
  Call Sha256InText
  Pop $1
  ${If} $1 != "1"
    Delete "$0"
    Push "o arquivo baixado não confere com o SHA-256 esperado (${NODE_SHA256})"
    Return
  ${EndIf}

  ; Extrai num diretório de passagem: uma falha não deixa node pela metade.
  DetailPrint "Extraindo o Node.js..."
  ; O NSIS não traz extrator de zip; o tar.exe (bsdtar) do Windows 10 1803+ lê
  ; zip. $SYSDIR é SysWOW64 num instalador 32-bit; Sysnative cobre esse caso.
  StrCpy $1 "$SYSDIR\tar.exe"
  ${IfNot} ${FileExists} "$1"
    StrCpy $1 "$WINDIR\Sysnative\tar.exe"
  ${EndIf}
  ${IfNot} ${FileExists} "$1"
    Push "tar.exe não encontrado: o Windows 10 1803 ou superior é necessário para extrair o Node.js"
    Return
  ${EndIf}
  RMDir /r "$LOCALAPPDATA\dish\node.new"
  CreateDirectory "$LOCALAPPDATA\dish\node.new"
  ExecWait '"$1" -xf "$0" -C "$LOCALAPPDATA\dish\node.new"' $2
  ${If} $2 != "0"
    RMDir /r "$LOCALAPPDATA\dish\node.new"
    Push "não foi possível extrair o Node.js (tar retornou $2)"
    Return
  ${EndIf}

  IfFileExists "$LOCALAPPDATA\dish\node.new\${NODE_ZIP}\node.exe" nodeExtractOk
    RMDir /r "$LOCALAPPDATA\dish\node.new"
    Push "o arquivo extraído não contém node.exe"
    Return
  nodeExtractOk:

  DetailPrint "Movendo o Node.js para $LOCALAPPDATA\dish\node..."
  RMDir /r "$LOCALAPPDATA\dish\node"
  Rename "$LOCALAPPDATA\dish\node.new\${NODE_ZIP}" "$LOCALAPPDATA\dish\node"
  RMDir /r "$LOCALAPPDATA\dish\node.new"
  Delete "$0"
  IfFileExists "$LOCALAPPDATA\dish\node\node.exe" nodeRenameOk
    Push "não foi possível mover o Node.js para o destino"
    Return
  nodeRenameOk:

  DetailPrint "Adicionando o Node.js ao PATH do usuário..."
  Call UpdateUserPath
  Pop $1
  ${If} $1 != "ok"
    Push "$1"
    Return
  ${EndIf}

  StrCpy $NodeOk "1"
  StrCpy $NodeExe "$LOCALAPPDATA\dish\node\node.exe"
  Push "ok"
FunctionEnd

Function InstallPi
  ${Do}
    Call InstallPiOnce
    Pop $0
    ${If} $0 == "ok"
      ${Break}
    ${EndIf}
    DetailPrint "Falha ao instalar o Pi: $0"
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Não foi possível instalar o Pi.$\r$\n$\r$\n$0$\r$\n$\r$\nTentar de novo? Em Cancelar, esta etapa é pulada e o Dish abre o assistente de primeira execução." /SD IDCANCEL IDRETRY piRetry
    DetailPrint "Instalação do Pi pulada."
    ${Break}
    piRetry:
  ${Loop}
FunctionEnd

Function InstallPiOnce
  InitPluginsDir
  ${If} $NodeExe == ""
    ; Pode ter sido instalado agora há pouco ou existir fora do PATH atual.
    Call DetectDeps
  ${EndIf}
  ${If} $NodeOk != "1"
    Push "Node.js ${NODE_MIN} ou superior não encontrado"
    Return
  ${EndIf}

  ${GetParent} "$NodeExe" $1
  StrCpy $2 "$1\npm.cmd"
  IfFileExists "$2" piNpmOk
    Push "npm não encontrado ao lado de $NodeExe"
    Return
  piNpmOk:

  ; Mesmos argumentos de src/installation.rs. O npm.cmd precisa achar o node:
  ; o `set "PATH=..."` dentro do cmd expande %PATH% no próprio cmd, então não
  ; há limite de 1024 caracteres do NSIS nem mudança no PATH do instalador.
  ; A saída do npm vai para um arquivo, não para o registro: ela pode conter
  ; credencial de registry privado (mesma regra do app); só as linhas de erro
  ; são mostradas.
  DetailPrint "Instalando o Pi em $LOCALAPPDATA\dish\pi (npm)..."
  nsExec::ExecToStack `cmd /C "set "PATH=$1;%PATH%" && "$1\npm.cmd" install -g --ignore-scripts --no-audit --no-fund --prefix "$LOCALAPPDATA\dish\pi" ${PI_PACKAGE} > "$PLUGINSDIR\npm.log" 2>&1"`
  Pop $2
  Pop $3
  ${If} $2 != "0"
    DetailPrint "npm terminou com status $2"
    nsExec::ExecToLog `findstr /B /C:"npm error code" /C:"npm error errno" /C:"npm ERR! code" /C:"npm ERR! errno" "$PLUGINSDIR\npm.log"`
    Pop $3
    Delete "$PLUGINSDIR\npm.log"
    Push "npm terminou com status $2 (veja o código de erro no registro acima)"
    Return
  ${EndIf}
  Delete "$PLUGINSDIR\npm.log"

  IfFileExists "$LOCALAPPDATA\dish\pi\pi.cmd" piInstalled
    Push "o npm terminou, mas pi.cmd não foi encontrado em $LOCALAPPDATA\dish\pi"
    Return
  piInstalled:

  nsExec::ExecToLog 'cmd /C "set "PATH=$1;%PATH%" && "$LOCALAPPDATA\dish\pi\pi.cmd" --version"'
  Pop $2
  ${If} $2 != "0"
    Push "o Pi foi instalado mas não respondeu a --version"
    Return
  ${EndIf}

  DetailPrint "Pi instalado em $LOCALAPPDATA\dish\pi"
  StrCpy $PiOk "1"
  StrCpy $PiExe "$LOCALAPPDATA\dish\pi\pi.cmd"
  Push "ok"
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\BUILD.json"
  Delete "$INSTDIR\install-pi-windows.ps1"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\licenses"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Desinstalar ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Dish"

  ; Preferências, sessões e a instalação do Pi ficam onde estão: desinstalar o
  ; Dish não pode apagar conversas nem mexer no Pi.
  ; O Node em %LOCALAPPDATA%\dish\node e a entrada no PATH também ficam, pelo
  ; mesmo motivo.
SectionEnd
