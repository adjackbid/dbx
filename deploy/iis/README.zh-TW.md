# 在 IIS 上部署 DBX Web（Windows，不用 Docker）

把 DBX Web 當成原生 Windows 執行檔、掛在 IIS 後面。就程式碼本身來說這條路是可行的
—— `crates/dbx-web` 是自帶 axum 的獨立伺服器，同時提供 HTTP API 與前端靜態檔 —— 但它
**不是官方發佈的建置目標**：

- Release 只發佈 **Linux (musl)** 的 `dbx-web` 執行檔。
- Windows 的 release 發的是 `dbx.exe`，那是 Tauri **桌面版**，不含 web server。
- 官方唯一打包好的 Web 部署方式是 Docker image。

所以 IIS 部署等於要自己從這個 repo 編出 `dbx-web.exe`，而且每次升版都要重跑一次。

## 部署結構

```
D:\dbx\
  bin\dbx-web.exe      target\release\dbx-web.exe
  static\              dist\（前端建置產物）      -> DBX_STATIC_DIR
  data\                執行期狀態、dbx.db、驅動程式 -> DBX_DATA_DIR
  logs\                HttpPlatformHandler 的 stdout log
```

兩種掛法：

| 方案 | 誰負責跑 `dbx-web.exe` | 適用情境 |
| --- | --- | --- |
| **A. HttpPlatformHandler**（建議） | IIS 以子程序方式啟動 | 元件最少，單一站台 |
| **B. ARR + URL Rewrite** | 你自己裝的 Windows 服務 | 需要程序不受 IIS App Pool 回收影響 |

## 快速開始

`deploy.bat`（薄包裝，實作在 `deploy.ps1`）把下面第 2～4 節自動化：

```bat
deploy.bat                                   :: 建置並佈署到 D:\dbx
deploy.bat -DeployRoot E:\apps\dbx            :: 換佈署目錄
deploy.bat -NoSqlCipher -SkipInstall          :: 跳過 vendored OpenSSL 建置
deploy.bat -SkipBuild -SkipIis                :: 只重新佈署既有建置產物
deploy.bat -Mode arr -Binding http/*:8080:    :: ARR 反向代理模式
deploy.bat -?                                 :: 完整說明
```

它會建置前端與 `dbx-web.exe`、佈署 `bin\`、`static\`、`data\`、`logs\`、`site\`，
依所選模式把範本套用成 `site\web.config`，接著在**以系統管理員身分執行**時建立
應用程式集區、解鎖 DBX 需要的組態區段、設定目錄 ACL，最後在 `127.0.0.1:4295` 對
佈署好的執行檔做一次冒煙測試。

`data\` 永遠不會被刪除或覆蓋；已存在的 `site\web.config` 除非加 `-Force` 否則會保留。
沒有系統管理員權限時，腳本仍會完成佈署，並印出要你手動執行的 `appcmd` 與 `icacls`
指令。

repo 裡的 `web.config` 是含 `{{DBX_ROOT}}`、`{{DBX_PORT}}`、`{{DBX_BASE_PATH}}`
代碼的範本，`deploy.ps1` 會代入並產生 `site\web.config`。

## 1. 先決條件

建置機：

- **Visual Studio Build Tools**，需含 *Desktop development with C++*（MSVC 編譯器，
  以及 **`vcvars64.bat`**、`VC\Tools\MSVC\<ver>\include`、`lib\x64`）與 Windows 10/11 SDK。
  VS 安裝若缺這些檔案，C 相依套件（openssl、zstd、sqlite）編不起來。`deploy.ps1`
  會在建置前檢查這兩者並指出缺哪個元件 —— 注意：機器上若有另一個「裝一半」的
  Visual Studio，即使有好的那份也可能讓建置失敗，所以檢查會回報它實際採用的安裝。
- **Strawberry Perl** 與 **NASM** —— 預設的 `sqlite-sqlcipher` 功能會編 vendored OpenSSL，
  需要這兩者。
- Rust（MSVC toolchain）、Node.js 22、pnpm。
- 請在 **x64 Native Tools Command Prompt**（先跑 `vcvars64.bat`）內建置，讓 `INCLUDE`
  與 `LIB` 對 OpenSSL 的 `nmake` 生效。`deploy.ps1` 會自己呼叫 `vcvars64.bat`，
  只有手動建置時才需要你處理。

伺服器：

- IIS 8+。
- WebSocket Protocol 功能（`/api/redis/pubsub/ws` 會用到）：
  `dism /online /enable-feature /featurename:IIS-WebSockets`。
- 方案 A：[HttpPlatformHandler](https://www.iis.net/downloads/microsoft/httpplatformhandler)。
- 方案 B：Application Request Routing 3.0+ 與 URL Rewrite 2.x。

## 2. 建置

`deploy.bat` 會跑完本節所有步驟。以下是在 repo 內手動執行的等效指令：

```powershell
# 前端 -> dist\
pnpm install --frozen-lockfile
pnpm build

# 後端 -> target\release\dbx-web.exe
cargo build --release -p dbx-web
```

Vite 的 base 預設是 `./`，產出的 `dist\` 用相對路徑，所以掛在網站根目錄或子路徑都行，
不需要重新建置。

若 vendored OpenSSL 建不起來（缺 Perl/NASM，或 `vcvars64.bat` 無法建立的開發環境），可以只拿掉
SQLCipher 功能。DBX 會失去「開啟 SQLCipher 加密的 SQLite 檔案」能力，它自己的 `dbx.db` 與其他
功能都不受影響。**MSVC 仍然必要** —— bundled SQLite 與 zstd 也是 C 程式碼：

```powershell
cargo build --release -p dbx-web --no-default-features `
  --features duckdb-sidecar,mq-admin,system-fonts
```

接著複製檔案：

```powershell
New-Item -ItemType Directory -Force D:\dbx\bin,D:\dbx\static,D:\dbx\data,D:\dbx\logs
Copy-Item target\release\dbx-web.exe D:\dbx\bin\
Copy-Item dist\* D:\dbx\static\ -Recurse
```

執行 DBX 的身分需要 `data` 目錄的寫入權（存 `dbx.db`、密碼雜湊、下載的驅動程式與受管 JRE）：

```powershell
# 方案 A
icacls D:\dbx\data /grant "IIS AppPool\dbx-web":(OI)(CI)M /T
# 方案 B（服務帳號，請換成你自己的）
icacls D:\dbx\data /grant "NT AUTHORITY\LOCAL SERVICE":(OI)(CI)M /T
```

`stdoutLogEnabled="true"` 時，`D:\dbx\logs` 也要給相同權限。

## 3. 環境變數

方案 A 寫在 `httpPlatform` 元素裡，方案 B 寫在服務定義裡。詳見
[Web API reference](../../docs/content/docs/web-api.mdx)。

| 變數 | 必要性 | 用途 |
| --- | --- | --- |
| `DBX_DATA_DIR` | **IIS 上必設** | 資料目錄。不設會 fallback 到 `$HOME\.dbx-web`；IIS worker process 沒有 `HOME`，會落到 worker 的當前目錄。 |
| `DBX_STATIC_DIR` | **必設** | 前端建置產物目錄（`dist\`）。不設就只提供 API。 |
| `DBX_PORT` | 方案 A 用 `%HTTP_PLATFORM_PORT%`；方案 B 用 `4224` | 監聽埠。 |
| `DBX_PASSWORD` | 建議設定 | 初始密碼。或改用首次啟動的設定頁，設定會寫入資料目錄。 |
| `DBX_DISABLE_PASSWORD=1` | 否 | 完全關閉密碼保護，僅限可信任的內網。 |
| `DBX_PUBLIC_BASE_PATH` | 子路徑才需要 | 例如 `/dbx`，見[子路徑部署](#6-子路徑部署)。 |
| `DBX_MAX_UPLOAD_MB` | 否 | 上傳上限，預設 1024 MB。IIS 的 `maxAllowedContentLength` 要不小於它。 |
| `DBX_AGENT_DIR` | 否 | 驅動／Agent 目錄，預設 `<DBX_DATA_DIR>\agents`。 |
| `RUST_LOG` | 否 | 例如 `dbx_web=info,tower_http=info`。 |

**沒有 `DBX_JAVA_BIN` 要設。** 那是 Docker image 建置時寫死的；Windows 上應用程式會自己在
`<DBX_DATA_DIR>\agents\jre-21` 解析受管 JRE，由 UI 的 **Driver Manager** 下載安裝。JDBC 類
資料庫（Oracle、DB2、SAP HANA 等）需要這次下載成功，或預先準備好 `agents` 目錄。

## 4. 方案 A —— HttpPlatformHandler

1. 建立一個站台（或應用程式），實體路徑 `D:\dbx\site`，把產生的 `web.config`
   放進去 —— 可以讓 `deploy.bat` 產生，或複製 [`web.config`](web.config) 後自己
   取代 `{{DBX_ROOT}}` / `{{DBX_BASE_PATH}}` 代碼。
2. 應用程式集區：**.NET CLR version = No Managed Code**、**Managed pipeline mode = Integrated**。
3. 在集區的 *Advanced Settings* 關掉回收與閒置關閉，否則 IIS 會定期終止 DBX，所有已登入
   的 session（存在記憶體）都會斷掉：
   - *Idle Time-out (minutes)* = `0`
   - *Regular Time Interval (minutes)* = `0`
   - *Disable Overlapped Recycle* = `True`
4. `web.config` 裡的 `requestTimeout="02:00:00"` 很重要：HttpPlatformHandler 預設是
   `00:02:00`，會把長時間的匯出、匯入、傳輸與慢查詢砍掉。
5. 若站台回 HTTP 500.19，把 `<remove name="WebDAV" />` 與 `<remove name="WebDAVModule" />`
   兩行刪掉 —— 沒安裝 WebDAV 功能時這兩行會失敗。

## 5. 方案 B —— ARR 反向代理 + Windows 服務

1. 用 [WinSW](https://github.com/winsw/winsw) 之類的 wrapper 把 `dbx-web.exe` 裝成服務
   （它是 console 應用程式，單靠 `sc.exe create` 無法託管）。DBX 的環境變數寫在服務定義裡，
   並讓服務帳號能存取 `D:\dbx\data`。
2. 在伺服器層級啟用 ARR proxy：IIS 管理員 → 伺服器節點 → *Application Request Routing
   Cache* → *Server Proxy Settings* → 勾選 *Enable proxy*。調整 **Time-out (seconds)** 並
   清除 *Response buffer*（或設 `responseBufferLimit="0"`），讓 SSE 端點能即時串流。
3. `<proxy>` 區段預設是鎖住的。若想放在 `web.config` 而不是在 IIS 管理員設定，先解鎖一次：

   ```powershell
   & $env:windir\system32\inetsrv\appcmd.exe unlock config /section:system.webServer/proxy
   ```

4. 建立站台，把產生的 `web.config` 放進去 —— 可以讓 `deploy.bat -Mode arr` 產生，
   或複製 [`web.config.arr-proxy`](web.config.arr-proxy) 後取代 `{{DBX_PORT}}` 代碼 ——
   若有改 `DBX_PORT`，記得同步改 rewrite 目標。
5. `dbx-web` 綁的是 `0.0.0.0` 而非 loopback，所以 `4224` 埠在網路上是通的。請用 Windows
   防火牆封鎖 inbound TCP 4224，只讓 IIS 能連。

## 6. 子路徑部署

要發佈在 `https://example.com/dbx/`：

1. 在 IIS 建立 `/dbx` 應用程式，且**保留**前綴。方案 B 的 `{R:1}` 已經保留；不要再加規則
   把 `/dbx` 去掉。
2. 設 `DBX_PUBLIC_BASE_PATH=/dbx`。伺服器會以 `/dbx/api/...` 提供 API，並把 `dbx_session`
   cookie 範圍限定在 `/dbx`。
3. 前端不需要重新建置：`dist\` 用相對路徑，前端由 `location.pathname` 加
   `import.meta.env.BASE_URL` 推算 base path。只有在你要非相對路徑的建置時，才需要在
   `pnpm build` 前設 `VITE_DBX_BASE_PATH`（或 `DBX_PUBLIC_BASE_PATH`）。
4. 瀏覽時請保留 `/dbx/` 的結尾斜線，這個 URL 形式由伺服器特別處理。

直接掛在網站根目錄可以完全避開這些事，風險最低。

## 7. 驗證

自己把佈署好的執行檔跑起來，就能看到 IIS 會代理到什麼：

```powershell
$env:DBX_STATIC_DIR = "D:\dbx\static"; $env:DBX_DATA_DIR = "D:\dbx\data"; $env:DBX_PORT = "4224"
D:\dbx\bin\dbx-web.exe
```

以下是用這個 repo 實際編出的 `dbx-web.exe` 量到的結果（根目錄、未設
`DBX_PUBLIC_BASE_PATH`）：

| 請求 | 狀態 | 內容 |
| --- | --- | --- |
| `GET /` | 200 | `index.html`，`text/html` |
| `GET /assets/index-*.js` | 200 | `text/javascript` |
| `GET /favicon.png` | 200 | `image/png` |
| `GET /api/auth/check` | 200 | `{"authenticated":false,"required":false,"setup_required":true,"user":null}` |
| `GET /api/version` | 200 | `{"version":"0.5.81"}` |
| `GET /some/deep/route` | **404** | `index.html`，`text/html` |

設 `DBX_PUBLIC_BASE_PATH=/dbx` 時：

| 請求 | 狀態 |
| --- | --- |
| `GET /dbx/` 與 `GET /dbx` | 200 |
| `GET /dbx/api/auth/check` | 200 |
| `GET /api/auth/check`（沒帶前綴） | 404 |
| `GET /` | 404 |

關於第一張表最後一列與結尾斜線，有兩點要知道：

- 深層連結會回 **404 但 body 是 SPA 外殼**。瀏覽器照樣渲染，導覽沒問題，但這些請求會在
  IIS log 裡記成 404。
- `/dbx/` 的結尾斜線由伺服器特別處理；掛在子路徑時請保留它。

接著在瀏覽器登入、建立連線、跑一次查詢，並**明確檢查串流功能**（Server-Sent Events）：
表格匯出的進度應該是持續更新，而不是最後一次跳完。若有在用 Redis Pub/Sub，也請測一次，
它會走到 WebSocket 路徑。

要看的 log：`D:\dbx\logs\stdout*.log`（方案 A）或服務的 log，以及
`%SystemDrive%\inetpub\logs\LogFiles\W3SVC*`。

## 8. 疑難排解

| 症狀 | 原因 |
| --- | --- |
| 站台回 HTTP 500.19 | 組態區段被鎖，或 handler 不存在。解鎖區段，例如方案 A 用 `appcmd unlock config /section:system.webServer/httpPlatform`、方案 B 用 `/section:system.webServer/proxy`，並確認 `handlers` 已解鎖：`appcmd unlock config /section:system.webServer/handlers`。若沒安裝 WebDAV 功能，請刪掉 `<remove name="WebDAV" />` / `<remove name="WebDAVModule" />` 兩行。 |
| HTTP 502.3 / 500，`httpPlatform` 相關錯誤 | `processPath` 錯，或 `startupTimeLimit` 太短；看 `D:\dbx\logs\stdout*.log`。 |
| 頁面開得起來但 API 404 | 沒設 `DBX_STATIC_DIR`（只提供 API），或 `DBX_PUBLIC_BASE_PATH` 與瀏覽器網址不一致。 |
| 大約每 20 分鐘就被登出 | 方案 A 的 App Pool 閒置逾時或回收還開著 —— session 存在記憶體。 |
| 每次 App Pool 回收就被登出 | 方案 A 的正常行為；改用方案 B。 |
| 長匯出／匯入約 2 分鐘就失敗 | `httpPlatform requestTimeout`，或 ARR proxy 的 timeout。 |
| SSE 進度一次全部出現 | 回應緩衝還開著；保留 `responseBufferLimit="0"` 並關掉 ARR 的 response buffer。 |
| 大檔上傳回 413 | 調高 `requestLimits.maxAllowedContentLength`（可在 `web.config` 設定）。`uploadReadAheadSize` **不能**按站台設定，要在伺服器層級：`appcmd set config /section:serverRuntime /uploadReadAheadSize:2147483647`。 |
| 連線存得起來但寫不進磁碟 | `DBX_DATA_DIR` 的 ACL 沒給 App Pool／服務帳號。 |
| 深層連結（例如 `/dbx/connections`）在 IIS log 裡是 404 | 這是預期行為：SPA fallback 會用 404 狀態回傳應用程式外殼（app shell）。瀏覽器正常渲染，不要用 URL Rewrite 去「修」它，那會破壞 API 路由。 |
| JDBC 類資料庫連不上 | 受管 JRE 尚未安裝；開 Driver Manager 安裝，或預先準備 `<DBX_DATA_DIR>\agents`。 |
| `deploy.bat` 停在「MSVC C++ headers are missing」 | 它選到的 Visual Studio 沒有 `VC\Tools\MSVC\<ver>\include`。請修復或安裝 *Desktop development with C++*；腳本會優先採用 `vswhere` 回報、且真的含有 VC tools 元件的那份安裝。 |

## 9. 預設被鎖住的組態區段

手動改 `web.config` 前值得先知道（以預設的 `applicationHost.config` 實測）：

| 區段 | 能否寫在 `web.config` | 說明 |
| --- | --- | --- |
| `handlers`、`modules` | 可以 | `applicationHost.config` 尾端的 `<location path="" overrideMode="Allow">` 區塊有重新宣告。 |
| `requestFiltering` | 可以 | 宣告為 `overrideModeDefault="Allow"`。 |
| `httpPlatform`（方案 A） | 通常可以 | 廠商區段；`deploy.bat` 會解鎖，也可自行執行 `appcmd unlock config /section:system.webServer/httpPlatform`。 |
| `proxy`（方案 B） | 解鎖後才可以 | 宣告為 `Deny`。用 `appcmd unlock config /section:system.webServer/proxy` 解鎖。 |
| `webSocket` | 不行 | 宣告為 `Deny`，而且預設已是 `enabled="true"`；只有 Windows 功能裝不裝的問題。 |
| `serverRuntime` | 不行 | 宣告為 `Deny` 且 `allowDefinition="AppHostOnly"`。`uploadReadAheadSize` 要在伺服器層級設定。 |

## 10. 維運要點

- 每次升版前備份 `D:\dbx\data`，裡面有連線設定、歷史紀錄與密碼雜湊。它**不含**目標資料庫
  的業務資料。
- 升版 = 停站台／服務 → 換掉 `bin\dbx-web.exe` 與 `static\` → 再啟動。`dbx.db` 會在啟動時
  自動遷移。
- 用 IIS 提供 HTTPS，`DBX_PASSWORD` 請用長且唯一的密碼。session cookie 是 `HttpOnly` +
  `SameSite=Lax`，但**沒有** `Secure`，所以不要把純 HTTP 曝露給不可信網路。
- 請審查驅動程式與 plugin 的來源；任何丟進資料目錄的東西都會被伺服器載入或執行。

另見 [Production Safety](../../docs/content/docs/production-safety.mdx) 與
[Web API reference](../../docs/content/docs/web-api.mdx)。

## 11. 複製到另一台伺服器

佈署好的資料夾是自帶的：目標伺服器不需要 Rust、Node、Docker 或 .NET runtime。要複製的約 60 MB：

```
<root>\bin\      dbx-web.exe      （約 38 MB）
<root>\static\   前端建置產物      （約 22 MB）
<root>\site\     web.config
<root>\logs\     請建立；HttpPlatformHandler 的 stdout log 目錄
```

**不要**複製 `<root>\data\`：裡面是來源機器的狀態（`dbx.db`、下載的驅動程式、受管 JRE）。
新的伺服器第一次啟動會自己建立。只有你確實要搬移既有連線與歷史紀錄時才複製它。

```powershell
robocopy D:\dbx \\target\D$\dbx /E /XD data
```

目標伺服器的需求：

| 需求 | 原因 |
| --- | --- |
| Windows 10/11 或 Server 2016+ | 執行檔連結了 Universal CRT（`api-ms-win-crt-*.dll`），這些版本已內建。 |
| **Visual C++ 2015-2022 可轉散發套件（x64）** | 執行檔匯入 `VCRUNTIME140.dll` —— 已用 `dumpbin /dependents` 實測。沒裝程序起不來。 |
| IIS 的 *WebSocket Protocol* 功能 | `/api/redis/pubsub/ws` 需要：`dism /online /enable-feature /featurename:IIS-WebSockets` |
| IIS 的 HttpPlatformHandler | 只有方案 A 需要。 |
| ARR + URL Rewrite | 只有方案 B 需要。 |

方案 A 不需要動防火牆：HttpPlatformHandler 給應用程式一個動態埠，對外只有 IIS 自己的
80/443 繫結。

請給 App Pool 身分 `<root>\data` 與 `<root>\logs` 的修改權（[第 2 節](#2-建置)），
並確認它能讀取 `<root>\bin` 與 `<root>\static` —— robocopy 不加 `/COPYALL` 不會帶 ACL。

路徑：目標機器若用相同的絕對路徑（例如同為 `D:\dbx`），產生的 `web.config` **不用改**。
否則請改 `site\web.config` 裡的四個路徑值 —— `processPath`、`stdoutLogFile`、
`DBX_STATIC_DIR`、`DBX_DATA_DIR` —— 或在建置機先重新產生再複製：

```bat
deploy.bat -DeployRoot E:\dbx -SkipBuild -SkipIis -Force
```

接 IIS 之前，先在目標機器證明執行檔能跑：

```bat
set DBX_STATIC_DIR=D:\dbx\static
set DBX_DATA_DIR=D:\dbx\data
set DBX_PORT=4224
D:\dbx\bin\dbx-web.exe
```

`http://localhost:4224` 應該出現登入頁。缺 DLL、權限不足都會在這裡立刻現形，可以把
執行期問題跟 IIS 設定問題分開。最後接[第 4 節](#4-方案-a--httpplatformhandler)或
[第 5 節](#5-方案-b--arr-反向代理--windows-服務)。

之後升版就簡化成：停站台 → 換掉 `bin\dbx-web.exe` 與 `static\` → 再啟動。`data\` 保留。
