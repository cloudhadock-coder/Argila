# Argila

IDE para Object Pascal/Delphi escrita em Rust: editor de units, visualizador de formulários `.dfm`
e arquitetura preparada para usar os compiladores oficiais (DCC32/DCC64) de uma instalação
licenciada do Delphi. Funciona offline e não distribui nenhum arquivo da Embarcadero.

O nome vem da paleta "Earthy Developer" (café, madeira, argila, bronze, areia).

## Rodar

```sh
cargo run              # debug (dependências compiladas com opt-level 2, a UI fica fluida)
cargo run --release    # sem janela de console
cargo test
```

Configurações: `%APPDATA%\Argila\settings.toml`. Atalhos personalizados: `%APPDATA%\Argila\keymap.json`.

## Arquitetura

| Módulo | Papel | Depende de GUI? |
|---|---|---|
| `app/` | estado da aplicação e execução de comandos (menu, barra, atalho e botão passam pelo mesmo caminho) | sim |
| `ui/` | só desenha e devolve pedidos: `toolbar`, `explorer`, `inspector`, `console`, `statusbar`, `welcome`, `dialogs` | sim |
| `shortcuts/` | `Command`, `KeyChord`, `Keymap` (perfil Delphi 12, JSON, conflitos) | só os tipos de tecla do egui |
| `settings/` | configurações persistentes em TOML, com `.bak` se o arquivo estiver corrompido | não |
| `filesystem/` | pasta de configuração e gravação atômica (temporário + rename) | não |
| `diagnostics/` | mensagens do painel Messages | não |
| `themes/` | paleta terrosa, visuais do egui, Segoe UI/Consolas do Windows | sim |

Módulos das próximas fases: `editor/`, `syntax/`, `projects/`, `dfm/` (parser, designer, componentes),
`compiler/` (`CompilerBackend`, `dcc32`, `dcc64`), `autocomplete/`, `navigation/`.

**Bibliotecas:** `eframe`/`egui` 0.31 (GUI), `serde` + `toml` + `serde_json`. Dev: `tempfile`.

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

### Atalhos do perfil padrão

Confirmados no docwiki da Embarcadero: **Replace = Ctrl+R / Ctrl+H** e **Open Project = Ctrl+F11**.
Os demais seguem o keymap Default do Delphi e ainda devem ser conferidos em
*Tools ▸ Options ▸ Editor Options ▸ Key Mappings* de um Delphi 12. O **Ctrl+O** para abrir arquivo vem
da especificação: no Delphi, Ctrl+O é prefixo de combinações (Ctrl+O+O etc.), que ainda não existem aqui.

## Limitações conhecidas

- Ainda não abre, edita nem salva arquivos (isso entra na Fase 2); a área central só tem a aba Welcome.
- Os painéis podem ser escondidos e redimensionados, mas não reposicionados (docking entra depois).
- Não há editor visual de atalhos; a personalização é feita pelo `keymap.json`.

## Próximos passos

1. **Fase 2, editor:**
   - widget próprio com buffer em rope (`ropey`) que desenha só as linhas visíveis;
   - leitura de bytes com detecção de BOM/UTF-8/cp1252 (`encoding_rs`), preservando CRLF;
   - salvamento atômico, undo/redo, lexer Pascal (portado do `dfm2html`), busca e substituição.
2. **Fase 3:** leitura de `.dpr`/`.dproj` sem alterá-los, Project Explorer, relação `.pas` ↔ `.dfm`.
3. **Fase 4:** parser de DFM (porte do `tools/dfmviewer/dfm2html.py` do sistemahorus, com herança visual),
   designer somente leitura e Object Inspector sincronizado.
