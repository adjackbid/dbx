# DBX Web on IIS (Windows, no Docker)

Run the DBX Web server as a native Windows binary behind IIS. This is a supported
*shape* for the code base — `crates/dbx-web` is a self-contained axum server that
serves both the HTTP API and the frontend assets — but it is **not a published
build target**:

- Releases ship `dbx-web` binaries for Linux (musl) only.
- The Windows releases ship `dbx.exe`, the Tauri **desktop** app. It does not
  contain the web server.
- The Docker image is the only officially packaged Web deployment.

So an IIS deployment means building `dbx-web.exe` yourself from this repository.
Expect to re-run these steps for every DBX upgrade.

A Traditional Chinese version of this guide is available in
[`README.zh-TW.md`](README.zh-TW.md).

## Deployment shape

```
D:\dbx\
  bin\dbx-web.exe      target\release\dbx-web.exe
  static\              dist\ (frontend build output)   -> DBX_STATIC_DIR
  data\                runtime state, dbx.db, drivers  -> DBX_DATA_DIR
  logs\                HttpPlatformHandler stdout log
```

Two ways to host it:

| Option | Who runs `dbx-web.exe` | Use when |
| --- | --- | --- |
| **A. HttpPlatformHandler** (recommended) | IIS, as a child process | Fewest moving parts; single site |
| **B. ARR + URL Rewrite** | A Windows service you install | You need the process to survive IIS app pool recycling |

## Quick start

`deploy.bat` (a thin wrapper around `deploy.ps1`) automates sections 2 to 4 below:

```bat
deploy.bat                                   :: build and stage to D:\dbx
deploy.bat -DeployRoot E:\apps\dbx            :: stage somewhere else
deploy.bat -NoSqlCipher -SkipInstall          :: skip the vendored OpenSSL build
deploy.bat -SkipBuild -SkipIis                :: restage an existing build only
deploy.bat -Mode arr -Binding http/*:8080:    :: ARR reverse-proxy mode
deploy.bat -?                                 :: full help
```

It builds the frontend and `dbx-web.exe`, stages `bin\`, `static\`, `data\`, `logs\`
and `site\`, writes `site\web.config` from the template for the selected mode, and then —
when run **elevated** — creates the app pool, unlocks the configuration sections DBX
needs, sets the directory ACLs, and smoke-tests the staged binary on `127.0.0.1:4295`.

`data\` is never deleted or overwritten, and an existing `site\web.config` is kept
unless you pass `-Force`. Without elevation the script still stages everything and
prints the exact `appcmd` and `icacls` commands to run as an administrator.

The committed `web.config` files are templates with `{{DBX_ROOT}}`, `{{DBX_PORT}}` and
`{{DBX_BASE_PATH}}` tokens; `deploy.ps1` substitutes them into `site\web.config`.

## 1. Prerequisites

Build machine:

- **Visual Studio Build Tools** with *Desktop development with C++* (MSVC compiler
  and the **`vcvars64.bat`** / `VC\Tools\MSVC\<ver>\include` + `lib\x64` files) and a
  Windows 10/11 SDK. A Visual Studio install that is missing these cannot compile
  the C dependencies. `deploy.ps1` checks for both before building and tells you which
  component is missing — note that a second, partially installed Visual Studio can
  break the build even when a good one exists, so the check reports the install it
  will actually use.
- **Strawberry Perl** and **NASM** — required by the vendored OpenSSL build that the
  default `sqlite-sqlcipher` feature pulls in.
- Rust (MSVC toolchain, `rustup target` default), Node.js 22, pnpm.
- Build from an **x64 Native Tools Command Prompt** (`vcvars64.bat`) so `INCLUDE`
  and `LIB` are set for the OpenSSL `nmake` build. `deploy.ps1` calls `vcvars64.bat`
  itself, so you only need to do this by hand for manual builds.

Server:

- IIS 8+.
- WebSocket Protocol feature (`/api/redis/pubsub/ws`; install with
  `dism /online /enable-feature /featurename:IIS-WebSockets`).
- Option A: [HttpPlatformHandler](https://www.iis.net/downloads/microsoft/httpplatformhandler).
- Option B: Application Request Routing 3.0+ and URL Rewrite 2.x.

## 2. Build

`deploy.bat` runs everything in this section. The manual equivalent, from a repository
checkout:

```powershell
# Frontend -> dist\
pnpm install --frozen-lockfile
pnpm build

# Backend -> target\release\dbx-web.exe
cargo build --release -p dbx-web
```

The Vite base path defaults to `./`, so the generated `dist\` uses relative asset
URLs and can be mounted at the root or under a subpath without rebuilding.

If the vendored OpenSSL build cannot run (missing Perl/NASM, or a developer
environment that `vcvars64.bat` cannot set up), drop only the SQLCipher feature. DBX
loses the ability to open SQLCipher-encrypted SQLite *files*; its own `dbx.db` and
everything else keep working. The MSVC toolchain is still required — the bundled
SQLite and zstd sources are C as well:

```powershell
cargo build --release -p dbx-web --no-default-features `
  --features duckdb-sidecar,mq-admin,system-fonts
```

Then copy:

```powershell
New-Item -ItemType Directory -Force D:\dbx\bin,D:\dbx\static,D:\dbx\data,D:\dbx\logs
Copy-Item target\release\dbx-web.exe D:\dbx\bin\
Copy-Item dist\* D:\dbx\static\ -Recurse
```

Grant the identity that runs DBX modify rights on the data directory — it stores
`dbx.db`, the password hash, downloaded drivers, and the managed JRE:

```powershell
# Option A
icacls D:\dbx\data /grant "IIS AppPool\dbx-web":(OI)(CI)M /T
# Option B (service account, adjust to yours)
icacls D:\dbx\data /grant "NT AUTHORITY\LOCAL SERVICE":(OI)(CI)M /T
```

`D:\dbx\logs` needs the same treatment when `stdoutLogEnabled="true"`.

## 3. Environment variables

Set these in the `httpPlatform` element (option A) or in the service definition
(option B). See [Web API reference](../../docs/content/docs/web-api.mdx).

| Variable | Required | Purpose |
| --- | --- | --- |
| `DBX_DATA_DIR` | **Yes on IIS** | Data directory. Without it DBX falls back to `$HOME\.dbx-web`; IIS worker processes have no `HOME`, so the fallback resolves to the worker's current directory. |
| `DBX_STATIC_DIR` | **Yes** | Directory holding the frontend build (`dist\`). Without it only the API is served. |
| `DBX_PORT` | Option A: `%HTTP_PLATFORM_PORT%`; Option B: `4224` | Listen port. |
| `DBX_PASSWORD` | Recommended | Initial password. Alternatively use the first-run setup page, which writes to the data directory. |
| `DBX_DISABLE_PASSWORD=1` | No | Disable password protection. Only for a trusted local network. |
| `DBX_PUBLIC_BASE_PATH` | Only for subpaths | e.g. `/dbx`. See [Sub-path deployment](#6-sub-path-deployment). |
| `DBX_MAX_UPLOAD_MB` | No | Upload limit, default 1024 MB. Keep the IIS `maxAllowedContentLength` at or above it. |
| `DBX_AGENT_DIR` | No | Driver/agent directory, defaults to `<DBX_DATA_DIR>\agents`. |
| `RUST_LOG` | No | e.g. `dbx_web=info,tower_http=info`. |

There is no `DBX_JAVA_BIN` to set. Docker sets it at image build time; on Windows
the app resolves its own managed JRE under `<DBX_DATA_DIR>\agents\jre-21`, installed
from **Driver Manager** in the UI. JDBC-based database types (Oracle, DB2, SAP HANA,
and similar) need that download to succeed, or a pre-seeded `agents` directory.

## 4. Option A — HttpPlatformHandler

1. Create a site (or application) with physical path `D:\dbx\site` and put the
   generated `web.config` there — either let `deploy.bat` write it, or copy
   [`web.config`](web.config) and replace the `{{DBX_ROOT}}` / `{{DBX_BASE_PATH}}`
   tokens yourself.
2. Application pool: **.NET CLR version = No Managed Code**, **Managed pipeline
   mode = Integrated**.
3. In *Advanced Settings* for the app pool, disable recycling and idle shutdown,
   otherwise IIS periodically terminates DBX and every logged-in session (sessions
   live in memory) is lost:
   - *Idle Time-out (minutes)* = `0`
   - *Regular Time Interval (minutes)* = `0`
   - *Disable Overlapped Recycle* = `True`
4. `requestTimeout="02:00:00"` in the shipped `web.config` matters: the
   HttpPlatformHandler default is `00:02:00`, which kills long exports, imports,
   transfers, and slow queries.
5. If the site returns HTTP 500.19, delete the `<remove name="WebDAV" />` and
   `<remove name="WebDAVModule" />` lines — they fail when the WebDAV feature is
   absent.

## 5. Option B — ARR reverse proxy + Windows service

1. Install `dbx-web.exe` as a service with a wrapper such as
   [WinSW](https://github.com/winsw/winsw) (it is a console app; `sc.exe create`
   alone cannot host it). Set the DBX environment variables in the service
   definition and give the service account access to `D:\dbx\data`.
2. Enable ARR proxy at the server level: IIS Manager → server node →
   *Application Request Routing Cache* → *Server Proxy Settings* → check
   *Enable proxy*. Adjust **Time-out (seconds)** and clear *Response buffer* (or set
   `responseBufferLimit="0"`) so SSE endpoints stream.
3. The `<proxy>` section is locked by default. To keep it in `web.config` instead of
   configuring it in IIS Manager, unlock it once:

   ```powershell
   & $env:windir\system32\inetsrv\appcmd.exe unlock config /section:system.webServer/proxy
   ```

4. Create the site, put the generated `web.config` there — either let
   `deploy.bat -Mode arr` write it, or copy [`web.config.arr-proxy`](web.config.arr-proxy)
   and replace the `{{DBX_PORT}}` token — and point the rewrite target at your
   service port if you changed `DBX_PORT`.
5. `dbx-web` binds `0.0.0.0`, not loopback, so port `4224` is reachable from the
   network. Block inbound TCP 4224 with Windows Firewall so only IIS can reach it.

## 6. Sub-path deployment

To publish at `https://example.com/dbx/`:

1. Create the IIS application at `/dbx` and **keep** the prefix in the forwarded
   path. Option B's `{R:1}` already does this; do not add a rewrite that strips
   `/dbx`.
2. Set `DBX_PUBLIC_BASE_PATH=/dbx`. The server then serves API routes as
   `/dbx/api/...` and scopes the `dbx_session` cookie to `/dbx`.
3. No frontend rebuild is needed: `dist\` uses relative asset URLs, and the frontend
   derives its base path from `location.pathname` plus `import.meta.env.BASE_URL`.
   Set `VITE_DBX_BASE_PATH` (or `DBX_PUBLIC_BASE_PATH`) before `pnpm build` only if
   you want a non-relative build.
4. Keep the trailing slash when browsing `/dbx/`; that URL form is handled
   explicitly by the server.

Deploying at the site root avoids all of this and is the lower-risk choice.

## 7. Verify

Start the staged binary yourself to see exactly what IIS will proxy to:

```powershell
$env:DBX_STATIC_DIR = "D:\dbx\static"; $env:DBX_DATA_DIR = "D:\dbx\data"; $env:DBX_PORT = "4224"
D:\dbx\bin\dbx-web.exe
```

Measured against a real `dbx-web.exe` built from this repository (root path, no
`DBX_PUBLIC_BASE_PATH`):

| Request | Status | Body |
| --- | --- | --- |
| `GET /` | 200 | `index.html`, `text/html` |
| `GET /assets/index-*.js` | 200 | `text/javascript` |
| `GET /favicon.png` | 200 | `image/png` |
| `GET /api/auth/check` | 200 | `{"authenticated":false,"required":false,"setup_required":true,"user":null}` |
| `GET /api/version` | 200 | `{"version":"0.5.81"}` |
| `GET /some/deep/route` | **404** | `index.html`, `text/html` |

With `DBX_PUBLIC_BASE_PATH=/dbx`:

| Request | Status |
| --- | --- |
| `GET /dbx/` and `GET /dbx` | 200 |
| `GET /dbx/api/auth/check` | 200 |
| `GET /api/auth/check` (no prefix) | 404 |
| `GET /` | 404 |

Two things worth knowing about the last row of the first table and the trailing
slash:

- Deep links return HTTP **404 with the app shell**. The browser still renders the
  SPA, so navigation works, but deep-link requests appear as 404s in the IIS log.
- `/dbx/` with the trailing slash is handled explicitly by the server; keep it when
  you mount the app under a subpath.

Then, in the browser: log in, create a connection, run a query, and check the
streaming features (`Server-Sent Events`) explicitly — a table export should show
progress continuously rather than in one jump at the end. Also verify Redis Pub/Sub
if you use it, which exercises the WebSocket path.

Logs to inspect: `D:\dbx\logs\stdout*.log` (option A) or the service log, plus
`%SystemDrive%\inetpub\logs\LogFiles\W3SVC*`.

## 8. Troubleshooting

| Symptom | Cause |
| --- | --- |
| HTTP 500.19 on the site | A locked configuration section or a handler that does not exist. Unlock the section, for example `appcmd unlock config /section:system.webServer/httpPlatform` (option A) or `/section:system.webServer/proxy` (option B), and make sure `handlers` is unlocked: `appcmd unlock config /section:system.webServer/handlers`. Also delete the `<remove name="WebDAV" />` / `<remove name="WebDAVModule" />` lines if the WebDAV feature is absent. |
| HTTP 502.3 / 500 with `httpPlatform` errors | `processPath` wrong, or `startupTimeLimit` too short; read `D:\dbx\logs\stdout*.log`. |
| Page loaded but API calls 404 | `DBX_STATIC_DIR` unset (API-only) or `DBX_PUBLIC_BASE_PATH` mismatch between server and browser URL. |
| Login lost every ~20 minutes | App pool idle timeout or recycling still enabled (option A) — sessions are in memory. |
| Login lost after every app pool recycle | Expected with option A; use option B. |
| Long exports/imports fail at ~2 minutes | `httpPlatform requestTimeout`, or the ARR proxy timeout. |
| SSE progress arrives all at once | Response buffering enabled; keep `responseBufferLimit="0"` and disable the ARR response buffer. |
| 413 on large uploads | Raise `requestLimits.maxAllowedContentLength` (allowed in `web.config`). `uploadReadAheadSize` is *not* configurable per site — set it at the server level: `appcmd set config /section:serverRuntime /uploadReadAheadSize:2147483647`. |
| Connection saved but cannot write to disk | ACLs on `DBX_DATA_DIR` for the app pool / service account. |
| Deep links (for example `/dbx/connections`) return 404 in the IIS log | Expected: the SPA fallback serves the app shell with a 404 status. Browsers render it normally. Do not "fix" it with a URL Rewrite, which would break API routing. |
| JDBC database types fail to connect | Managed JRE not installed; open Driver Manager and install it, or pre-seed `<DBX_DATA_DIR>\agents`. |
| `deploy.bat` stops with "MSVC C++ headers are missing" | The Visual Studio install it selected has no `VC\Tools\MSVC\<ver>\include`. Repair or install *Desktop development with C++*; the script prefers the install reported by `vswhere` that actually has the VC tools component. |

## 9. Configuration sections that are locked by default

Useful to know before editing `web.config` by hand (verified against a default
`applicationHost.config`):

| Section | Usable in `web.config` | Note |
| --- | --- | --- |
| `handlers`, `modules` | Yes | Re-declared inside the trailing `<location path="" overrideMode="Allow">` block of `applicationHost.config`. |
| `requestFiltering` | Yes | Declared `overrideModeDefault="Allow"`. |
| `httpPlatform` (option A) | Usually yes | Vendor section; `deploy.bat` unlocks it, and you can unlock it yourself with `appcmd unlock config /section:system.webServer/httpPlatform`. |
| `proxy` (option B) | Only after unlocking | Declared `Deny`. Unlock with `appcmd unlock config /section:system.webServer/proxy`. |
| `webSocket` | No | Declared `Deny`, and the default is already `enabled="true"`. Only the Windows feature matters. |
| `serverRuntime` | No | Declared `Deny` with `allowDefinition="AppHostOnly"`. Set `uploadReadAheadSize` at the server level. |

## 10. Operations

- Back up `D:\dbx\data` before every upgrade; it holds connections, history, and
  the password hash. It does **not** contain business data from target databases.
- Upgrading = stop the site/service, replace `bin\dbx-web.exe` and `static\`, start
  again. `dbx.db` is migrated on startup.
- Serve HTTPS from IIS and keep the `DBX_PASSWORD` unique. The session cookie is
  `HttpOnly` and `SameSite=Lax` but not `Secure`, so do not expose plain HTTP to
  untrusted networks.
- Review the driver and plugin sources; anything dropped into the data directory is
  executed or loaded by the server.

See [Production Safety](../../docs/content/docs/production-safety.mdx) and the
[Web API reference](../../docs/content/docs/web-api.mdx).

## 11. Copying the deployment to another server

The staged folder is self-contained: no Rust, Node, Docker, or .NET runtime is needed
on the target. Copy roughly 60 MB:

```
<root>\bin\      dbx-web.exe      (~38 MB)
<root>\static\   frontend build   (~22 MB)
<root>\site\     web.config
<root>\logs\     create it; the HttpPlatformHandler stdout log target
```

Do **not** copy `<root>\data\`: it holds the source machine's state (`dbx.db`,
downloaded drivers, managed JRE). A fresh server creates its own on first start. Copy
it only when you deliberately want to migrate existing connections and history.

```powershell
robocopy D:\dbx \\target\D$\dbx /E /XD data
```

Target server requirements:

| Requirement | Why |
| --- | --- |
| Windows 10/11 or Server 2016+ | The binary links the Universal CRT (`api-ms-win-crt-*.dll`), built into these versions. |
| **Visual C++ 2015-2022 Redistributable (x64)** | The binary imports `VCRUNTIME140.dll` — verified with `dumpbin /dependents`. Without it the process does not start. |
| IIS with the *WebSocket Protocol* feature | Needed by `/api/redis/pubsub/ws`: `dism /online /enable-feature /featurename:IIS-WebSockets` |
| IIS with HttpPlatformHandler | Option A only. |
| ARR + URL Rewrite | Option B only. |

No firewall change is needed for option A: HttpPlatformHandler gives the app a dynamic
port and IIS exposes only its own 80/443 bindings.

Give the app pool identity modify rights on `<root>\data` and `<root>\logs`
([section 2](#2-build)), and make sure it can read `<root>\bin` and `<root>\static` —
robocopy without `/COPYALL` does not carry ACLs.

Paths: when the target uses the same absolute path (for example `D:\dbx`), the generated
`web.config` needs no edit. Otherwise change the four path values in `site\web.config` —
`processPath`, `stdoutLogFile`, `DBX_STATIC_DIR`, `DBX_DATA_DIR` — or regenerate it on
the build machine before copying:

```bat
deploy.bat -DeployRoot E:\dbx -SkipBuild -SkipIis -Force
```

Before wiring IIS, prove the binary starts on the target:

```bat
set DBX_STATIC_DIR=D:\dbx\static
set DBX_DATA_DIR=D:\dbx\data
set DBX_PORT=4224
D:\dbx\bin\dbx-web.exe
```

`http://localhost:4224` should show the login page. Missing-DLL and permission failures
show up here immediately, which separates runtime problems from IIS configuration
problems. Finish with [section 4](#4-option-a--httpplatformhandler) or
[section 5](#5-option-b--arr-reverse-proxy--windows-service).

Upgrades then reduce to: stop the site, replace `bin\dbx-web.exe` and `static\`, start
again. Keep `data\`.
