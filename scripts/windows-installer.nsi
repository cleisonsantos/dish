; Instalador do Dish para Windows (NSIS).
;
; Escolha por um EXE/NSIS em vez de MSI: o fluxo pedido na issue #13 é um
; assistente (avançar/avançar/concluir) com detecção do Pi, e o NSIS faz isso
; sem exigir administrador — a instalação é por usuário, em %LOCALAPPDATA%.
;
; O Dish continua sendo só o cliente: quem instala o Pi é o instalador oficial
; do Pi, executado aqui apenas se a pessoa marcar a opção na página de
; verificação. O assistente de primeira execução do Dish cobre o mesmo passo
; depois, caso ninguém marque nada.
;
; Compilar:  makensis /DVERSION=0.1.0-alpha.3 /DVERSIONNUM=0.1.0.3 /DSRCDIR=..\.. scripts\windows-installer.nsi

Unicode true

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

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
Page custom PiCheckPage PiCheckLeave
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Executar o Dish"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "English"

var PiCheckbox
var PiFound
var RunPiInstaller

; --------------------------------------------------------------- verificação do Pi

Function PiCheckPage
  ; `where pi` responde 0 quando o Pi está no PATH do usuário.
  nsExec::ExecToStack 'cmd /C where pi'
  Pop $0
  Pop $1
  ${If} $0 == "0"
    StrCpy $PiFound "1"
  ${Else}
    StrCpy $PiFound "0"
  ${EndIf}

  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 24u "O Dish é um cliente do Pi: ele precisa do Pi instalado para funcionar."
  Pop $0

  ${If} $PiFound == "0"
    ${NSD_CreateLabel} 0 30u 100% 40u "O Pi não foi encontrado neste computador.$\r$\nVocê pode instalá-lo agora com o instalador oficial (uma janela do PowerShell será aberta) ou deixar para o assistente do Dish na primeira execução."
    Pop $0
    ${NSD_CreateCheckbox} 0 76u 100% 12u "Executar o instalador oficial do Pi agora"
    Pop $PiCheckbox
  ${Else}
    ${NSD_CreateLabel} 0 30u 100% 24u "Pi encontrado neste computador."
    Pop $0
  ${EndIf}

  nsDialogs::Show
FunctionEnd

Function PiCheckLeave
  ${If} $PiFound == "0"
    ${NSD_GetState} $PiCheckbox $RunPiInstaller
  ${Else}
    StrCpy $RunPiInstaller "0"
  ${EndIf}
FunctionEnd

Function .onInstSuccess
  ${If} $RunPiInstaller == ${BST_CHECKED}
    ; Instalador oficial do Pi, como no assistente do Linux: nada é escondido
    ; da pessoa e o Dish não reimplementa a instalação.
    ExecShell "open" "powershell.exe" "-NoProfile -ExecutionPolicy Bypass -Command $\"irm https://pi.dev/install.ps1 | iex$\""
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------------- seções

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

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} "O aplicativo, a licença, o aviso de terceiros e o pacote de licenças das dependências."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_STARTMENU} "Atalhos no menu Iniciar, incluindo o desinstalador."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} "Um atalho na área de trabalho."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\BUILD.json"
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
SectionEnd
