# Argila

IDE para Object Pascal/Delphi escrita em Rust: editor de units, visualizador de formulários `.dfm`
e arquitetura preparada para usar os compiladores oficiais (DCC32/DCC64) de uma instalação
licenciada do Delphi. Funciona offline e não distribui nenhum arquivo da Embarcadero.

O nome vem da paleta "Earthy Developer" (café, madeira, argila, bronze, areia).

## Rodar

```sh
cargo run                         # debug (dependências com opt-level 2, o crate com 1)
cargo run --release -- Unit1.pas  # abre arquivos passados na linha de comando
cargo test
```

Também dá para arrastar arquivos para a janela ou abrir pelos recentes da aba Welcome.
`ARGILA_PERF=1` imprime no stderr o tempo médio e máximo de quadro a cada 2 s.

Configurações: `%APPDATA%\Argila\settings.toml`. Atalhos personalizados: `%APPDATA%\Argila\keymap.json`.

## Arquitetura

| Módulo | Papel | Depende de GUI? |
|---|---|---|
| `app/` | estado da aplicação e execução de comandos (menu, barra, atalho e botão passam pelo mesmo caminho) | sim |
| `ui/` | só desenha e devolve pedidos: `toolbar`, `explorer`, `inspector`, `console`, `statusbar`, `welcome`, `dialogs` | sim |
| `shortcuts/` | `Command`, `KeyChord`, `Keymap` (perfil Delphi 12, JSON, conflitos) | só os tipos de tecla do egui |
| `settings/` | configurações persistentes em TOML, com `.bak` se o arquivo estiver corrompido | não |
| `filesystem/` | pasta de configuração, gravação atômica e `encoding` (BOM, UTF-8, ANSI, CRLF/LF) | não |
| `editor/` | `buffer` (rope + desfazer), `document` (abrir/salvar), `ops`, `search`; `view` é o widget | só `view` |
| `syntax/` | lexers Pascal e DFM por linha, com estado entre linhas calculado sob demanda | não |
| `diagnostics/` | mensagens do painel Messages | não |
| `themes/` | paleta terrosa, visuais do egui, Segoe UI/Consolas do Windows | sim |

Módulos das próximas fases: `projects/`, `dfm/` (parser, designer, componentes),
`compiler/` (`CompilerBackend`, `dcc32`, `dcc64`), `autocomplete/`, `navigation/`.

**Bibliotecas:** `eframe`/`egui` 0.31 (GUI), `ropey` (buffer), `encoding_rs` (ANSI), `rfd` (diálogo
nativo de abrir), `serde` + `toml` + `serde_json`. Dev: `tempfile`.

- **Por que egui e não Slint:** editor de código, designer de DFM e grades do Object Inspector são
  desenho customizado; no modo imediato do egui isso sai direto pelo `Painter`.
- **Por que egui 0.31 e não a 0.37:** é a versão cuja API foi validada aqui. A atualização é uma
  tarefa à parte, guiada pelos erros do `cargo check`.

## Fase 1 (fundação), concluída

- Janela principal com tema terroso (paleta da especificação, mais um âmbar derivado para avisos).
- Menus File, Edit, Search, View, Project, Run, Tools e Help, além de uma faixa de ferramentas.
- Painel esquerdo (Project Explorer / Structure), painel direito (Object Inspector com Properties/Events
  e busca), painel inferior (Messages, Search Results, Diagnostics, Build Output, Console),
  abas de documento e barra de status.
- Painéis redimensionáveis e escondíveis (✕, menu View ou atalho); tamanho e posição são salvos.
- Configurações persistentes: janela, painéis, abas ativas, editor e recentes. Gravação atômica e
  adiada; arquivo inválido vira `.bak` e a IDE abre com os padrões.
- Atalhos centralizados com casamento exato de modificadores (Ctrl+S não dispara com Shift+Ctrl+S),
  perfil Delphi 12, `keymap.json` para sobrescrever comandos, exportação (Tools ▸ Export Keymap),
  detecção de conflitos e lista em Help ▸ Keyboard Shortcuts.
- Comando ainda não implementado **não simula execução**: a barra de status e o painel Messages dizem
  em que fase ele entra. Compile/Build/Run aparecem como indisponíveis por falta de compilador.
- 15 testes (unitários e de integração): configurações, gravação atômica, parse e exibição de atalhos,
  perfil sem conflitos, importação/exportação e recuperação de arquivos inválidos.

## Fase 2 (editor de código), concluída

- **Editor virtualizado próprio** (sem o `TextEdit` do egui): a cada quadro lê do rope, tokeniza e
  desenha só as linhas visíveis. Gutter com numeração, linha do cursor, seleção, rolagem horizontal
  e vertical, tabs expandidos.
- **Edição:**
  - mouse: clique, Shift+clique, duplo clique (palavra), triplo clique (linha), arrastar com rolagem
    automática;
  - teclado: setas, Ctrl+setas por palavra, Home inteligente, End, PageUp/Down, Ctrl+Home/End,
    Ctrl+A, Ctrl+Backspace/Delete;
  - área de transferência: recortar, copiar e colar, copiando com a quebra de linha do arquivo.
- **Indentação ao estilo Delphi:**
  - Enter repete a indentação e acrescenta um nível depois de `begin`, `try`, `then`, `do`, `var`,
    `uses` etc.;
  - Backspace dentro da indentação volta até a parada de tab;
  - Tab / Shift+Tab e Ctrl+Shift+I/U indentam e recuam blocos;
  - Ctrl+/ comenta linhas.
- **`.pas` e `.dfm`** (e `.dpr`, `.dpk`, `.inc`) em modo texto, com realce:
  - os lexers seguem as regras do `dfm2html`: comentários `//`, `{ }` e `(* *)`, diretivas `{$ }`,
    strings com `''` e `#13`, números `$FF`, strings multilinha `'''` do Delphi 12;
  - no DFM, `object`/`end`, classe, valores e blocos binários `{ }`.
- **Codificação**, detectada nesta ordem:
  1. BOM (UTF-8, UTF-16 LE/BE);
  2. UTF-8 válido com acentos;
  3. senão, o ANSI do Windows (`GetACP`), como faz o Delphi, sem presumir Windows-1252
     (`editor.default_encoding` muda isso).
  - A barra de status permite *reabrir com* outra codificação e *salvar com* outra codificação ou
    quebra de linha.
  - Arquivo só com ASCII continua ANSI: acentos digitados nele não viram UTF-8 escondido.
- **Quebras de linha:** CRLF/LF preservados; CR isolado volta idêntico; arquivo com CRLF e LF
  misturados é avisado no painel Messages.
- **Fidelidade:** todo arquivo aberto passa por um teste de ida e volta byte a byte. Se não bater,
  ele é marcado "com perda" e salvar pede confirmação.
- **Desfazer/refazer:**
  - a digitação é agrupada por palavra, por pausa (1,5 s) e por quebra de linha;
  - um grupo nunca atravessa o ponto salvo, e voltar com desfazer até lá deixa o arquivo como "não
    modificado";
  - operações de várias edições (Replace All, indentar bloco) são um único desfazer.
- **Busca e substituição** (Ctrl+F / Ctrl+R / Ctrl+H):
  - busca incremental, contador "N of M", anterior/próxima (Shift+Enter / Enter / F3);
  - opções de maiúsculas (com acentos: "AÇÃO" acha "ação") e palavra inteira;
  - Replace e Replace All. Também Alt+G (ir para linha).
- **Segurança:**
  - gravação atômica;
  - diálogos para fechar ou sair com alterações, caractere sem representação na codificação
    (nada é gravado; oferece UTF-8 com BOM), arquivo alterado por outro programa e arquivo lido com
    perda;
  - `.dfm` binário (TPF0) e arquivos binários são recusados.
- **Indicadores:** ● na aba, "Modified", Ln/Col, codificação, quebra de linha e linguagem na barra de
  status.
- 36 testes novos (51 no total): codificações e todos os 256 bytes de Windows-1252 em ida e volta,
  outra code page ANSI (1250), BOMs, CRLF/LF/misto/CR isolado, abrir-editar-salvar-reabrir com
  acentos, caractere sem representação, alteração externa, arquivo binário, arquivo de 10 mil linhas,
  agrupamento de desfazer e ponto salvo, operações de edição, busca e lexers.

### Desempenho observado

Medido com o `Unit_Fl_Funcionario.pas` real (10.424 linhas, 458 KB, UTF-8 com BOM, CRLF) e uma cópia
convertida para Windows-1252, numa máquina Windows 11:

| Operação | Release | Debug |
|---|---|---|
| Abrir (ler, decodificar, montar o rope) | 5–8 ms | — |
| Tempo médio de quadro (rolar 150 páginas, buscar, digitar) | 0,7–2,2 ms | 1,6–4,6 ms |
| Pior quadro depois de carregado | ~6 ms | ~78 ms (busca de 458 KB a cada tecla) |
| 500 edições + 500 desfazer (teste de 10 mil linhas) | 0,6 ms | 0,9 ms |
| Salvar 10 mil linhas | 7 ms | 33 ms |

O primeiro quadro leva de 30 a 50 ms, por causa do carregamento das fontes do Windows.

### Atalhos do perfil padrão

Confirmados no docwiki da Embarcadero: **Replace = Ctrl+R / Ctrl+H** e **Open Project = Ctrl+F11**.
Os demais seguem o keymap Default do Delphi e ainda devem ser conferidos em
*Tools ▸ Options ▸ Editor Options ▸ Key Mappings* de um Delphi 12. O **Ctrl+O** para abrir arquivo vem
da especificação: no Delphi, Ctrl+O é prefixo de combinações (Ctrl+O+O etc.), que ainda não existem aqui.

## Limitações conhecidas

- Os painéis podem ser escondidos e redimensionados, mas não reposicionados (docking entra depois).
- Não há editor visual de atalhos; a personalização é feita pelo `keymap.json`.
- **Editor:**
  - sem seleção em coluna, múltiplos cursores, minimapa, code folding, destaque de `begin`/`end` e
    autocompletar (Fase 5);
  - sem suporte a IME (entrada de idiomas asiáticos);
  - sem quebra automática de linha;
  - caracteres largos (CJK) desalinham a grade monoespaçada.
- **Arquivos:**
  - abrir e salvar acontecem na thread da interface; é rápido (< 10 ms para 10 mil linhas), mas um
    disco de rede lento travaria a janela;
  - não há recuperação após queda (cópias automáticas de arquivos modificados);
  - a gravação atômica cria um arquivo novo, então atributos como somente leitura e ACLs
    personalizadas do original não são copiados.
- **Quebras e codificação:**
  - um arquivo com CRLF e LF misturados é salvo todo com a quebra predominante (com aviso);
  - "Reopen with" descarta o buffer, por isso só fica disponível sem alterações pendentes.
- **Busca:** a cada tecla, a busca varre o arquivo inteiro. Em release é imperceptível; em debug dá
  picos de dezenas de ms.
- **Acessibilidade:** o conteúdo do editor não é exposto à acessibilidade (AccessKit); menus, botões
  e barra de status são.

## Próximos passos

1. **Fase 3:** leitura de `.dpr`/`.dproj` sem alterá-los, Project Explorer, relação `.pas` ↔ `.dfm`.
2. **Fase 4:** parser de DFM (porte do `tools/dfmviewer/dfm2html.py` do sistemahorus, com herança visual),
   designer somente leitura e Object Inspector sincronizado.
