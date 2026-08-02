[English](README.md) · [Português](README.pt-BR.md) · **Español** · [简体中文](README.zh-CN.md)

# Rojo Manager

Una pequeña app de escritorio para ejecutar y supervisar varios procesos `serve` locales de
[Rojo](https://rojo.space) — uno por proyecto — desde una sola ventana, con logs en vivo por
proyecto y presencia en la bandeja del sistema, de modo que cerrar la ventana nunca detiene
tus serves.

Hecha con **Tauri 2 + React + TypeScript**. Interfaz cálida al estilo Claude, con temas
Claro/Oscuro/Sistema.

## Qué hace

- Añade / edita / elimina proyectos (nombre, carpeta, archivo de proyecto Rojo, puerto,
  args opcionales). Se guardan como JSON en el directorio de datos de la app del sistema
  (`%APPDATA%/com.rojomanager.app/projects.json` en Windows).
- Inicia/detiene cada proyecto de forma independiente — varias instancias de `rojo serve`
  corren en paralelo.
- La salida de cada proyecto se captura y se transmite a una consola de logs sin congelar la
  interfaz.
- Se niega a iniciar un proyecto que ya está corriendo, o uno cuyo puerto ocupa otro serve.
- Cerrar la ventana la oculta en la **bandeja del sistema** (los serves siguen vivos). Menú
  de la bandeja: Mostrar / Ocultar / Detener todos los serves / Salir. Salir termina todos
  los procesos hijos limpiamente.

`rojo` debe estar instalado y en tu `PATH` (por ejemplo vía
[Rokit](https://github.com/rojo-rbx/rokit)).

## Descarga

Toma el instalador de tu sistema desde la
[última release](https://github.com/ocauapaz/rojo-manager/releases/latest):

| SO | Archivo |
| --- | --- |
| Windows | `.msi` o `.exe` (NSIS) |
| macOS | `.dmg` |
| Linux | `.AppImage` o `.deb` |

Los binarios no están firmados, así que SmartScreen en Windows y Gatekeeper en macOS
avisarán en el primer arranque — "Más información → Ejecutar de todas formas", o clic
derecho → Abrir en macOS. Si prefieres no confiar en un binario, compila desde el código
(abajo).

## Compilar desde el código

Necesita [Node 20+](https://nodejs.org) y la [toolchain de Rust](https://rustup.rs), además
de las [dependencias de sistema de Tauri](https://tauri.app/start/prerequisites/) para tu SO.

```bash
git clone https://github.com/ocauapaz/rojo-manager.git
cd rojo-manager
npm install
npm run tauri build
```

Produce un ejecutable sin consola y un instalador en `src-tauri/target/release/` (el
bundle/instalador queda en `src-tauri/target/release/bundle/`). Instálalo (o ejecuta el
`.exe` directamente) y ábrelo como cualquier otra app.

## Desarrollo

```bash
npm install
npm run tauri dev      # ventana de desarrollo con recarga en caliente
```

## Estructura

| Ruta | Propósito |
| --- | --- |
| `src-tauri/src/lib.rs` | Gestión de procesos, persistencia, bandeja, ciclo de vida |
| `src/App.tsx` | Panel: lista lateral + detalle del proyecto + consola de logs |
| `src/components/` | `ProjectForm`, `LogPanel`, `ThemeToggle` |
| `src/api.ts` / `src/types.ts` | Envoltorios de comandos Tauri + tipos compartidos |
| `src/styles.css` | Tema (variables CSS; `:root[data-theme="dark"]`) |

## Releases

Empuja una etiqueta `v*` y GitHub Actions compila los bundles de Windows, macOS y Linux en
una release en **borrador**; edítala y publícala a mano.

```bash
npm version 0.1.1 --no-git-tag-version   # sube la versión en package.json
# sube también `version` en src-tauri/tauri.conf.json
git commit -am "chore: v0.1.1" && git tag v0.1.1 && git push --follow-tags
```

## Licencia

MIT — ver [LICENSE](LICENSE).
