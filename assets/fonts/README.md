# Fontes embutidas

O Dish carrega estas fontes no binário (`include_bytes!` em `src/main.rs`) para
que a leitura tenha o mesmo desenho em qualquer sistema. Ambas são distribuídas
sob a SIL Open Font License 1.1; o texto da licença acompanha cada família e é
copiado para `share/licenses/fonts/` nos pacotes.

| Família | Arquivos | Origem | Licença |
|---|---|---|---|
| Source Serif 4 (texto das respostas) | Regular, It, Semibold | https://github.com/adobe-fonts/source-serif/releases/tag/4.005R | `source-serif-4/LICENSE.md` (OFL-1.1, Reserved Font Name "Source") |
| Geist (interface) | Regular, Medium, SemiBold | https://github.com/vercel/geist-font/releases/tag/v1.9.2 | `geist/OFL.txt` (OFL-1.1) |

Os arquivos são os TTF estáticos originais, sem modificação (a OFL exige outro
nome para versões modificadas).
