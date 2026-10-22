# DCC32 / DCC64 - RAD Studio 37.0

Copia dos compiladores da instalacao local, reservada para integracao futura.
Origem: `C:\Program Files (x86)\Embarcadero\Studio\37.0\bin`.

- `bin/dcc32.exe`: gera codigo Win32.
- `bin/dcc64.exe`: gera codigo Win64.
- Ambos sao executaveis host x86; as DLLs acompanhantes vem da mesma pasta.
- `manifest.json` registra origem, tamanho, SHA-256 e imports externos.
- `radstudio_license_en.rtf` preserva os termos da instalacao de origem.

Nao ha chamada destes compiladores pela IDE, alteracao de PATH ou configuracao
de build do Argila. Os binarios e a licenca sao da Embarcadero.

Esta pasta nao e uma instalacao autonoma do Delphi. As configuracoes `.cfg`
foram preservadas e apontam para as bibliotecas Win32/Win64 da instalacao
original. RTL/VCL, DCUs, SDKs e credenciais de licenciamento nao foram copiados.
A futura integracao devera configurar caminhos de bibliotecas e recursos,
validar o licenciamento e tratar os diagnosticos e codigos de saida.

Para consultar os compiladores no PowerShell a partir da raiz do repositorio:

```powershell
& .\toolchains\delphi\37.0\bin\dcc32.exe '-?'
& .\toolchains\delphi\37.0\bin\dcc64.exe '-?'
```

## Verificacao nesta maquina

Em 2026-10-21, ambos os executaveis copiados foram iniciados com `-?` e
retornaram a mensagem `This version of the product does not support command
line compiling.` (codigo de saida 0). Portanto, a edicao/licenca instalada
nao permite compilacao pela linha de comando. A copia nao remove essa
restricao; a integracao futura depende de uma edicao/licenca compativel.
Nenhuma compilacao Delphi foi validada.
