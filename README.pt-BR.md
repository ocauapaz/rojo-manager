[English](README.md) · **Português** · [Español](README.es.md) · [简体中文](README.zh-CN.md)

# Rojo Manager

Um app de desktop enxuto para rodar e supervisionar vários processos `serve` do
[Rojo](https://rojo.space) — um por projeto — a partir de uma única janela, com logs ao vivo
por projeto e presença na bandeja do sistema, de modo que fechar a janela nunca derruba seus
serves.

Feito com **Tauri 2 + React + TypeScript**. Interface quente no estilo Claude, com temas
Claro/Escuro/Sistema.

## O que ele faz

- Adiciona / edita / remove projetos (nome, pasta, arquivo de projeto Rojo, porta, args
  opcionais). Guardado como JSON no diretório de dados do app do SO
  (`%APPDATA%/com.rojomanager.app/projects.json` no Windows).
- Inicia/para cada projeto de forma independente — várias instâncias de `rojo serve` rodam
  em paralelo.
- A saída de cada projeto é capturada e transmitida para um console de log sem travar a UI.
- Recusa iniciar um projeto já em execução, ou um cuja porta esteja ocupada por outro serve.
- Fechar a janela esconde na **bandeja do sistema** (os serves continuam). Menu da bandeja:
  Mostrar / Esconder / Parar Todos os Serves / Sair. Sair encerra todos os processos filhos
  de forma limpa.

O `rojo` precisa estar instalado e no seu `PATH` (por exemplo via
[Rokit](https://github.com/rojo-rbx/rokit)).

## Download

Pegue o instalador do seu sistema no
[release mais recente](https://github.com/ocauapaz/rojo-manager/releases/latest):

| SO | Arquivo |
| --- | --- |
| Windows | `.msi` ou `.exe` (NSIS) |
| macOS | `.dmg` |
| Linux | `.AppImage` ou `.deb` |

Os binários não são assinados, então o SmartScreen do Windows e o Gatekeeper do macOS vão
avisar na primeira execução — "Mais informações → Executar assim mesmo", ou clique com o
botão direito → Abrir no macOS. Se preferir não confiar em um binário, compile do código
(abaixo).

## Compilar do código

Precisa de [Node 20+](https://nodejs.org) e da [toolchain do Rust](https://rustup.rs), mais
as [dependências de sistema do Tauri](https://tauri.app/start/prerequisites/) para o seu SO.

```bash
git clone https://github.com/ocauapaz/rojo-manager.git
cd rojo-manager
npm install
npm run tauri build
```

Gera um executável sem console e um instalador em `src-tauri/target/release/` (o
bundle/instalador fica em `src-tauri/target/release/bundle/`). Instale (ou rode o `.exe`
direto) e abra como qualquer outro app.

## Desenvolvimento

```bash
npm install
npm run tauri dev      # janela de dev com hot reload
```

## Estrutura

| Caminho | Função |
| --- | --- |
| `src-tauri/src/lib.rs` | Gerência de processos, persistência, bandeja, ciclo de vida |
| `src/App.tsx` | Dashboard: lista lateral + detalhe do projeto + console de log |
| `src/components/` | `ProjectForm`, `LogPanel`, `ThemeToggle` |
| `src/api.ts` / `src/types.ts` | Wrappers dos comandos Tauri + tipos compartilhados |
| `src/styles.css` | Tema (variáveis CSS; `:root[data-theme="dark"]`) |

## Releases

Empurre uma tag `v*` e o GitHub Actions compila os bundles de Windows, macOS e Linux em um
release **rascunho**; edite e publique manualmente.

```bash
npm version 0.1.1 --no-git-tag-version   # sobe a versão no package.json
# suba também o `version` em src-tauri/tauri.conf.json
git commit -am "chore: v0.1.1" && git tag v0.1.1 && git push --follow-tags
```

## Licença

MIT — veja [LICENSE](LICENSE).
