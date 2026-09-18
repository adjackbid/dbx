# 🤝 Context Handoff

## Meta
- **exported_at**: 2026-09-15T18:10:53+08:00
- **exported_from**: GitHub Copilot CLI
- **session_id**: 845973a5-be63-458d-89b0-8b8e6fdcb4bd
- **supersedes**: `handoff.md` @ HEAD (the 2026-07-29 Schema Diff DDL task — recover with `git show HEAD:handoff.md`)

## Project
- **name**: dbx
- **stack**: Rust, TypeScript, Vue 3, Tauri, Axum, pnpm
- **root**: D:\Code_AI\dbx
- **package_manager**: pnpm
- **working tree**: uncommitted — `deploy/iis/` is untracked, `README.md` and `README.zh-CN.md` each have one added line

## Current Task
Build a Windows/IIS deployment path for **DBX Web that does not use Docker**, then prove it
works. IIS cannot host a native Rust binary directly, so the work was: confirm the shape is
viable, document it, automate it, and verify it against a real build.

The important framing: **this is not a supported release target.** `crates/dbx-web` is a
self-contained axum server (HTTP API + static SPA), but releases ship `dbx-web` binaries for
Linux (musl) only (`.github/workflows/release.yml`, job `static-browser`), and the Windows
releases ship `dbx.exe` — the Tauri desktop app, which contains no web server. The Docker
image is the only packaged Web deployment. So an IIS deployment requires building
`dbx-web.exe` locally, and the deliverable is tooling plus documentation, not a build-target
change.

## Progress
- [x] Confirmed the IIS shape is viable and found the exact constraints (runtime deps, locked config sections, websocket/SSE needs)
- [x] Added `deploy/iis/deploy.ps1` — build, stage, generate config, configure IIS, smoke test
- [x] Added `deploy/iis/deploy.bat` — thin wrapper, follows the repo's `dev-full.bat` → `.ps1` convention
- [x] Added `deploy/iis/web.config` (HttpPlatformHandler) and `deploy/iis/web.config.arr-proxy` (ARR + URL Rewrite) as token templates
- [x] Added `deploy/iis/README.md` and `deploy/iis/README.zh-TW.md` (11 sections each, kept in sync)
- [x] Linked the guide from `README.md` and `README.zh-CN.md`
- [x] Fixed 4 real bugs found while testing (see Key Decisions)
- [x] Verified every script branch that does not require elevation, using a stub HTTP server as `dbx-web.exe`
- [x] Built the real artifact and staged it: `cargo build --release -p dbx-web --no-default-features --features duckdb-sidecar,mq-admin,system-fonts` → 23m33s → `D:\dbx`
- [x] Verified the real binary serves the staged static build and the API, in both root and sub-path mode
- [ ] **IIS app pool / site creation / ACLs were never executed** — this session had no administrator rights, so the script printed the `appcmd`/`icacls` commands instead
- [ ] Default-feature build (SQLCipher enabled) was **not** built; only the `-NoSqlCipher` variant exists
- [ ] Nothing is committed yet
- [ ] Optional: verify `-C target-feature=+crt-static` to drop the VC++ redistributable requirement on target servers

## Verification Evidence
Real binary, real staged tree, measured — not inferred.

| Item | Result |
| --- | --- |
| `cargo check -p dbx-web` inside `vcvars64.bat` | passed in 12m51s, warnings only |
| `cargo build --release --no-default-features …` | 23m33s, `D:\dbx\bin\dbx-web.exe` = 38,238,208 bytes |
| Staged frontend | 893 files in `D:\dbx\static` |
| Generated config | `D:\dbx\site\web.config`, 3,504 bytes, HttpPlatformHandler mode, `DBX_PUBLIC_BASE_PATH` commented out |
| Script smoke test | `dbx-web served D:\dbx\static and /api/auth/check on 127.0.0.1:4295`, exit 0 |
| `GET /` | 200 `text/html` (3,030 bytes) |
| `GET /assets/index-*.js` | 200 `text/javascript` |
| `GET /api/auth/check` | 200 `{"authenticated":false,"required":false,"setup_required":true,"user":null}` |
| `GET /api/version` | 200 `{"version":"0.5.81"}` |
| `GET /some/deep/route` | **404** with the `index.html` body (content-length 3,030) |
| `DBX_PUBLIC_BASE_PATH=/dbx`: `/dbx/`, `/dbx`, `/dbx/api/auth/check` | 200, 200, 200 |
| `DBX_PUBLIC_BASE_PATH=/dbx`: `/api/auth/check`, `/` | 404, 404 |
| Stub-based branch tests | staging, XML validation, smoke-test pass **and** failure paths (exit 1, no hang, logs surfaced), `-DryRun`, `-Mode arr`, `-SubPath` normalisation, `web.config` idempotency (hand edit preserved) |
| Test leftovers | removed — no fake `target\release\dbx-web.exe`, no temp deploy roots, no orphaned processes |

The deployment lives **outside** the repo at `D:\dbx` (`bin\`, `static\`, `data\`, `logs\`,
`site\`) and is not tracked by git. `D:\dbx\data\dbx.db` exists because the verification runs
started the server; it is a fresh, unconfigured store (no password was set).

## Active Files
- `handoff.md` — this document
- `deploy/iis/deploy.ps1` — the whole deployment: toolchain preflight (locates a usable MSVC install and `vcvars64.bat`, validates `vcruntime.h` + Windows SDK), `pnpm` build, `cargo build` inside `vcvars64.bat`, staging, config generation with XML validation, app pool / unlock / site / ACL steps, smoke test on `127.0.0.1:4295`, exit 1 when verification fails
- `deploy/iis/deploy.bat` — entry point; forwards arguments to the script
- `deploy/iis/web.config` — HttpPlatformHandler template; tokens are `{{DBX_ROOT}}` and `{{DBX_BASE_PATH}}`
- `deploy/iis/web.config.arr-proxy` — ARR reverse-proxy template; token is `{{DBX_PORT}}`
- `deploy/iis/README.md` — 11 sections: shape, prerequisites, build, env vars, option A, option B, sub-paths, verify, troubleshooting, locked sections, copying to another server
- `deploy/iis/README.zh-TW.md` — Traditional Chinese mirror; keep the two in sync
- `README.md`, `README.zh-CN.md` — one added bullet under Documentation
- `crates/dbx-web/src/main.rs` — reference only, not modified. Source of truth for `DBX_*` env handling: `DBX_STATIC_DIR` fallback + SPA `not_found_service`, `DBX_PUBLIC_BASE_PATH` normalisation and the explicit `/dbx/` root route (issue #5518), `DBX_PORT` default 4224, bind address `0.0.0.0`, `DBX_DATA_DIR` falling back to `$HOME`
- `apps/desktop/vite.config.ts` — reference only. Base path defaults to `./`, which is why the staged frontend works at the root or under a sub-path without a rebuild

## Blocker
Nothing is blocking code work. Three residual risks to be explicit about:

1. **The elevation-only paths are untested.** `deploy.ps1`'s `appcmd add apppool`,
   `unlock config`, `add site`, `set app`, and `icacls` calls never ran. They are wrapped so a
   failure warns instead of aborting, and the non-elevated path prints the equivalent commands
   (that output *was* verified). Someone with administrator rights should run
   `deploy.bat` once on a real server and confirm the pool, site, and ACL results.
2. **The staged artifact cannot open SQLCipher-encrypted SQLite files** because it was built
   with `-NoSqlCipher`. That is a deliberate time-saving choice, not a defect. The default
   feature set needs the vendored OpenSSL build, which should now work on this machine
   (see Environment) but has not been re-run.
3. **`HttpPlatformHandler` was never exercised**, because it is not installed on this machine
   and installing it needs elevation. `requestTimeout`, `responseBufferLimit`, and WebSocket
   pass-through are configured from Microsoft's documented behaviour, not measured here.

## Key Decisions
- **Do not change build targets or CI.** IIS support stays opt-in tooling under `deploy/iis/`.
  Adding a Windows `dbx-web` release artifact is a product decision for the maintainers.
- **One source of truth for configuration.** The committed `web.config` files are templates
  and `deploy.ps1` substitutes the tokens. Never put a `{{TOKEN}}` inside an XML comment —
  string replacement hits comments too, and `--` inside a comment produces invalid XML. This
  bug happened during this session; the script's XML validation caught it.
- **Always build inside `vcvars64.bat` when one is available, including `-NoSqlCipher`.** cc-rs
  resolves the C toolchain itself and can select a Visual Studio install that is missing its
  C++ component; the dev-prompt environment pins it to the install whose `INCLUDE` was
  validated. `-NoSqlCipher` should only change the cargo feature flags, never the build shell.
  This was a real bug: the first version skipped vcvars in that mode, which would have failed
  on bundled SQLite and zstd.
- **Never touch `data\`.** `static\` is build output and is safe to mirror; `data\` holds
  `dbx.db`, connections, the password hash, downloaded drivers, and the managed JRE. The
  script refuses to clean anything whose leaf name is not `static`.
- **Do not ship a password placeholder.** The generated config leaves `DBX_PASSWORD` commented
  out and relies on the first-run setup page, so no known credential can end up in a config.
- **App pool idle timeout `0` and recycling disabled** for HttpPlatformHandler mode, because
  sessions live in memory and a recycle logs every user out.
- **The SPA deep-link 404 is expected, not a bug to fix.** `GET /any/deep/route` returns the app
  shell with status 404. Browsers render it; a URL Rewrite meant to "fix" it would break API
  routing. Documented in both READMEs.
- **`webSocket` and `serverRuntime` must not appear in `web.config`.** Verified locked on a
  default install; see Environment.

## Environment
Hard-won facts about this machine — several contradict earlier assumptions.

- **Platform**: Windows, IIS 10 installed and running (`W3SVC`), `appcmd.exe` present. This
  session ran **without administrator rights**, so no IIS configuration could be changed.
- **Visual Studio 18 Community at `D:\Program Files\Microsoft Visual Studio\18\Community` is
  incomplete**: no `vcvarsall.bat`, no `VC\Tools\MSVC\14.51.36231\include`, no `lib\x64`. Any C
  build that selects it dies with `vcruntime.h` or `limits.h` not found. `vswhere -requires
  Microsoft.VisualStudio.Component.VC.Tools.x86.x64` does *not* report it.
- **Visual Studio 2019 BuildTools at `C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools`
  is complete** (MSVC 14.29.30133) and is what the preflight selects:
  `...\VC\Auxiliary\Build\vcvars64.bat`.
- **Perl and NASM are present** (`C:\Strawberry\perl\bin\perl.exe`, `C:\Strawberry\c\bin\nasm.exe`).
  The previous handoff claimed `cargo check` failed because "perl is missing" — that diagnosis
  was wrong. The real cause was a missing `INCLUDE`/`LIB` environment (no developer prompt) plus
  cc-rs selecting the broken VS 18 install. `cargo check -p dbx-web` inside `vcvars64.bat`
  now succeeds.
- **Windows SDK 10.0.19041** lives on `D:\Windows Kits\10` (not under `C:\Program Files (x86)\Windows Kits`).
- **Default IIS configuration locks** (read from `applicationHost.config` and its trailing
  `<location path="" overrideMode="Allow">` block, which re-declares `modules` and `handlers`):
  `handlers` and `modules` are usable in `web.config`; `requestFiltering` is `Allow`;
  `webSocket` and `serverRuntime` are locked (`serverRuntime` is also `AppHostOnly`, which is why
  `uploadReadAheadSize` must be set at the server level); `proxy` (ARR) is locked and needs
  `appcmd unlock config /section:system.webServer/proxy`. `HttpPlatformHandler` is **not**
  installed here.
- **Runtime dependencies of the built exe** (`dumpbin /dependents`): system DLLs plus
  `VCRUNTIME140.dll` and the `api-ms-win-crt-*.dll` set. So a target server needs the Visual C++
  2015-2022 x64 redistributable, and Windows 10/11 or Server 2016+ for the Universal CRT.
- **Env vars that matter**: `DBX_DATA_DIR` must be set explicitly on IIS (the app falls back to
  `$HOME\.dbx-web`, and IIS worker processes have no `HOME`); `DBX_STATIC_DIR` must point at the
  staged frontend; `DBX_PORT` accepts `%HTTP_PLATFORM_PORT%`. There is **no** `DBX_JAVA_BIN` to
  set on Windows — Docker sets it at image build time, while Windows resolves a managed JRE under
  `<DBX_DATA_DIR>\agents\jre-<n>` via the in-app Driver Manager.
- **`dbx-web` binds `0.0.0.0`**, not loopback, so the ARR mode needs a firewall rule for the
  backend port.
- **Release profile is expensive**: `lto = true`, `codegen-units = 1`, `opt-level = "s"` in the
  root `Cargo.toml`; a release build is ~24 minutes. Do not assume it hung.
- Useful commands: `deploy\iis\deploy.bat -?`, `… -DryRun`, `… -SkipBuild -SkipIis`,
  `cargo check -p dbx-web`.

## Next Steps
1. On the target server, run `deploy.bat` from an elevated prompt (or execute the printed
   `appcmd`/`icacls` list) and point an IIS site at `<root>\site`; then create the password on
   the first-run page.
2. Rebuild without `-NoSqlCipher` if SQLCipher-encrypted SQLite support is needed: plain
   `deploy.bat`. Expect roughly 5–15 extra minutes for OpenSSL.
3. Commit `deploy/iis/` plus the two README link lines when the wording is settled.
4. Decide whether the project wants an official Windows `dbx-web` artifact; if yes, the release
   workflow needs a new job (today only `static-browser` builds `dbx-web`, and it is Linux musl).
5. Optional hardening: try `RUSTFLAGS=-C target-feature=+crt-static` (the Windows 7 job already
   uses it for the desktop app) so target servers no longer need the VC++ redistributable.

## For the Next AI
- Read `deploy/iis/README.md` first — §9 lists the locked IIS sections, §11 the copy-to-another-server
  recipe, §8 the troubleshooting table.
- Do not add `DBX_PASSWORD` to a committed config, do not "fix" the deep-link 404, and do not put
  `{{TOKEN}}` inside an XML comment.
- Do not modify `data\` on any existing deployment, and keep the app pool's idle timeout at `0`.
- If you change a template, run `deploy.bat -SkipBuild -SkipIis -Force` into a temp
  `-DeployRoot` and confirm the generated file parses as XML.
- The elevation-only code paths are the least certain part of `deploy.ps1`; treat a real elevated
  run on a server as the remaining acceptance test.
