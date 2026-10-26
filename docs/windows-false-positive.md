# Windows: investigação de falso positivo (#102)

## Evidências e limites

Fonte: [issue #102](https://github.com/cleisonsantos/dish/issues/102), consultada em
2026-10-25. Os hashes abaixo são transcritos da issue, não recalculados nesta tarefa.
O evento de 2026-10-22 10:47:14 (horário local) foi `Detected suspicious running
process`, sobre `%LOCALAPPDATA%\Programs\Dish\dish.exe`, versão 0.1.0-alpha.5.
O agente aparecia SECURE/Online, sem indicação de quarentena na captura descrita.
Ainda não sabemos a regra, versão do Windows/EDR ou ação efetiva do console.

| Artefato | SHA-256 informado |
| --- | --- |
| `dish-0.1.0-alpha.5-windows-x86_64-setup.exe` (5.683.917 bytes) | `38edda081feda1297398aec68d073239f277a167097e12dab864a0405f9821c5` |
| `dish.exe` extraído (~21 MB) | `fd304f5a2acc26b8a84734d82a58c911892d18a38f1fbbd27ae835ccfbe9479e` |

Segundo a issue, o checksum publicado confere, o Security Directory dos dois PEs
está vazio (sem Authenticode), e o asset tinha dois downloads naquele momento.
`BUILD.json`: target `x86_64-pc-windows-msvc`, rustc 1.99.1, versão
0.1.0-alpha.5, hash de Cargo.lock
`6c18eefa9e96bf92ed6e64406643fa7ef9642a4ef02daa68cfb7f01566c3bef0`.

É uma hipótese de falso positivo, não uma confirmação. Baixa prevalência,
ausência de assinatura e processos filhos (`node --version`, `git`, instalação
via `npm`) podem contribuir. O NSIS também iniciava PowerShell remoto com
`ExecutionPolicy Bypass` e `irm | iex`, marcado por padrão. Não há evidência
suficiente para atribuir a esse comando o alerta específico no processo Dish.
O produto que alertou foi SentinelOne, não Defender nem SmartScreen.

## Mitigações no código

- O NSIS deixa a opção do Pi desmarcada. Ao escolhê-la, abre o helper local
  `install-pi-windows.ps1`, também usado pelo botão de copiar comando no Windows.
- O helper baixa por HTTPS para um diretório temporário exclusivo, apresenta
  URL, caminho, SHA-256 e estado Authenticode, abre o arquivo no Bloco de Notas
  e aguarda seu fechamento. Exige a palavra exata `INSTALAR` e confere novamente
  o hash antes de executar o arquivo local. Recusa alteração durante a revisão,
  limpa o download ao terminar e mostra erros. Cancelar não executa o download.
- Não usa bypass, `iex` nem desbloqueio de arquivo. Políticas locais podem
  impedir tanto o helper quanto o script baixado; nesse caso, procure o
  administrador ou use a instalação via npm já oferecida pelo Dish.
- A assinatura do script baixado é informativa: não há pin de publisher nem
  hash confiável publicado incorporado. O hash local detecta alteração durante
  a revisão, mas não autentica o download. A pessoa precisa avaliar o conteúdo;
  o fluxo não protege contra um processo hostil executando como o mesmo usuário.
- `winresource`, que já era dependência, passa a incluir CompanyName
  (`Cleison Santos`, o publisher já usado no NSIS), FileVersion e ProductVersion,
  além de ProductName e FileDescription existentes. As versões vêm de Cargo
  (atualmente 0.1.0); o rótulo alpha do instalador continua vindo do CI.
  Nenhuma dependência ou entrada de Cargo.lock foi adicionada.

A busca também encontrou `curl -fsSL https://pi.dev/install.sh | sh` em
`src/installation.rs`: o lançamento Linux exige clicar em **Aceitar e abrir
instalador oficial**, e a alternativa manual exige copiar/executar no terminal.
Permanece um risco de executar conteúdo remoto sem inspeção, embora haja
consentimento visível; foi reportado aqui, sem refatorar o fluxo Linux.

## Assinatura opt-in no CI

Criar os dois secrets de Actions no repositório:

- `WINDOWS_CERT_PFX_BASE64`: PFX codificado em base64, com certificado de code
  signing confiável e sua chave privada (exatamente um certificado com chave).
- `WINDOWS_CERT_PASSWORD`: senha não vazia desse PFX.

Sem ambos, o build continua **sem assinatura**, com mensagem explícita. Se apenas
um estiver configurado, ou se importação, assinatura, timestamp ou verificação
falhar, o job falha: não publica silenciosamente um artefato supostamente assinado.
O helper importa em `Cert:\CurrentUser\My`, usa signtool do Windows SDK com SHA-256
e timestamp RFC 3161 (`http://timestamp.digicert.com`), verifica com
`signtool verify /pa /v` e remove PFX temporário e certificados/chaves em `finally`.
O runner hospedado é efêmero; uma interrupção abrupta pode impedir `finally`.

`dish.exe` é assinado antes de ser copiado ao payload. O instalador é assinado
após o NSIS e seu `.sha256` é recalculado sobre os bytes finais antes do upload.
Sem secrets, vale o checksum gerado pelo empacotador. O helper PowerShell e o
uninstaller não recebem assinatura neste fluxo. Certificados que exigem HSM ou
serviço remoto precisam de integração própria; estes secrets só suportam PFX.
Restrinja quem pode alterar/rodar workflows com secrets de assinatura. Assinar
não garante reputação imediata nem ausência de alertas comportamentais.

## Submissão ao SentinelOne

1. O administrador do endpoint deve abrir o evento no console, exportar regra,
   identificador da detecção, process tree, linhas de comando, hash no endpoint,
   usuário (redigido nos anexos públicos), versões do agente/Windows e ação
   tomada (alerta, kill ou quarentena). Comparar o hash com o release exato.
2. Abrir um caso pelo [suporte oficial SentinelOne](https://www.sentinelone.com/global-services/get-support-now/)
   usando a conta do cliente/time de segurança. Pedir análise de possível falso
   positivo e seguir o canal de envio de amostra indicado no caso.
3. Anexar os binários por canal autorizado, hashes, `.sha256`, `BUILD.json`,
   [release alpha.5](https://github.com/cleisonsantos/dish/releases/tag/v0.1.0-alpha.5),
   fonte e descrição: cliente GUI do Pi, filhos legítimos de descoberta e npm,
   instalação por usuário e atalhos. Distinguir o release original do novo build;
   novos builds/assinaturas terão hashes diferentes.
4. Guardar número do caso, resposta e regra corrigida. Reproduzir com a versão
   atualizada do agente em endpoint autorizado antes de encerrar a investigação.

Somente o dono/time autorizado pode acessar o console, liberar quarentena,
classificar o evento ou mudar políticas. Uma eventual exclusão deve depender da
análise e aprovação desse time, ter escopo mínimo (por exemplo, hash exato) e
revisão registrada. Não excluir pastas amplas nem desativar proteção para testar.
Nenhum ticket, envio de amostra ou alteração de política foi feito nesta tarefa.

## Submissão à Microsoft

Não há detecção Microsoft comprovada nesta issue. Se Defender/SmartScreen
sinalizar um artefato, tratar separadamente:

1. Acessar [Submit a file for malware analysis](https://www.microsoft.com/en-us/wdsi/filesubmission),
   autenticar-se e selecionar o perfil de desenvolvedor de software.
2. Selecionar o produto que realmente detectou o arquivo, indicar falso positivo
   e enviar o binário exato com a detecção, versão, hash e contexto do build.
3. Guardar o ID da submissão, acompanhar a análise e retestar com inteligência
   de segurança atualizada. Não usar uma resposta Microsoft como confirmação de
   que o SentinelOne foi corrigido.

Referência: [FAQ oficial para desenvolvedores](https://learn.microsoft.com/en-us/defender-xdr/developer-faq).

## não verificado

Nada disso foi executado em Windows real nesta tarefa. Os testes Linux e as
checagens de texto/YAML não validam NSIS, PowerShell, certificado ou EDR. Ainda
é preciso compilar no CI Windows com e sem secrets, conferir VERSIONINFO no PE,
validar as duas assinaturas e o checksum final, instalar em VM e testar cancelar,
aceitar, editar o download, erro de rede e bloqueio por política. O dono precisa
obter os detalhes do evento e confirmar o resultado com o SentinelOne. Não se
pode afirmar que o falso positivo foi resolvido.

## Validação local desta alteração

Executada em Linux, sem alterar condições de skip/ignore:

- `TMPDIR=/tmp cargo check --locked`: sucesso, `Finished dev profile` (2m06s).
- `TMPDIR=/tmp cargo test --locked`: `210 passed; 0 failed; 1 ignored`.
- `cargo clippy --locked --all-targets -- -D warnings`: sucesso,
  `Finished dev profile` (9m00s, incluindo espera pelo lock dos testes).
- `PATH=/usr/bin:$PATH python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/windows.yml'))"`:
  sucesso. O Python padrão do ambiente não tinha PyYAML; o Python do sistema tinha.
- `/tmp/bin/actionlint .github/workflows/windows.yml`: sucesso, sem diagnósticos.
- `PYTHONPATH=tests python3 -m unittest test_release_package.WindowsPackagingTests -v`:
  13 testes, todos aprovados. São verificações estruturais, não execução Windows.
- `python3 -m unittest discover -s tests -p 'test_release_package.py' -v`:
  32 testes, um erro e uma falha por ausência de `dpkg-deb`/`dpkg-query`,
  quatro skips preexistentes (artefato candidato e ferramentas Debian).
  Repetir com o Python do sistema deu o mesmo resultado. Nenhum teste foi
  removido ou relaxado; o teste que exigia checkbox pré-marcado foi atualizado
  para exigir opt-in e ganhou verificações contra execução remota direta.
- `git diff --check`: sucesso. `Cargo.toml` e `Cargo.lock` permanecem sem alterações.

O comando `gh issue view 102` falhou por conectividade com api.github.com; a issue
completa foi lida pelo conector GitHub. Não houve commit nem push.
