# M7 逐行對讀差異清單：RemotePairing 模式（子系統 11/9 之 RP 半部）

日期：2026-11-18。規格來源：補盤 `docs/m7-rp-inventory.md`
（RemotePairingKit 全源碼＋兩 gateway RP 段逐行＋idevice
crate 審計）。範圍：DESIGN §4 M7。本文件先記 FFI 重評估
（D1 重評估點）與補盤裁示，再記實作對照與 D 表。

## 1. FFI 重評估（DESIGN D1／§3.1）：維持 swift-bridge，不試點 uniffi

**結論：維持 swift-bridge 全線；mm-rp 不導入 uniffi。**

證據（皆為已落地且經 CI 驗證的現行機制，非推測）：

- M7 本身**零新回呼形狀**：RP 會話建立是 App→Rust 的
  async 命令路（pair-verify／隧道／RSD 皆由 Rust 主動
  連出），不需任何反向請求；事件面沿用 M4 的
  `on_status(seq, …)` 與 M6 的 `on_log` 單向 sink。
- M8（回呼最密集段）的三種回呼形狀在本庫已有同形先例：
  `onPinReceived`／`onReadyToPair` 是單向事件（＝
  on_status 形）；`onRequestPin` 是請求／回應對，而
  **完全同形的 resolveServicePort 已於 M2 落地並驗證**
  （`MmConnectionConfig.request_service_port(request_id,
  …)`＋oneshot 註冊表＋10 秒逾時回退，mm-ffi lib.rs）。
  PIN 只是把同一機制掛到 sink 面，無新原語需求。
- uniffi 試點的成本是實的（全部既有導出重寫、CI 新增
  bindgen 步、殼模組重排），而其 callback interface 優勢
  已被上述先例抵銷；「摩擦過高」的啟動條件拿不出具體
  證據（無任何回呼形狀在 swift-bridge 表達不出來）。
  依 §3.1 原文「若屆時 sink 模式摩擦過高，允許 mm-rp
  單獨試點」——條件不成立，不啟動。

FFI 面本段處置：**不新增 bridge 函數**。Swift 公開面中
RP 不帶來新操作形狀——start／fetchUDID／performHeartbeat／
安裝／DDI／除錯等全數經既有導出（mm_gateway_*／
mm_core_*）在 RP 模式下生效；埠面（mm_gateway_service_
port／set_port、mm_core_set_params 的 remote_pairing_
port）M1/M4 已備。綁定不變、無重生。此評估記為 D-M7-5。

## 2. 補盤裁示摘要（全文見 inventory）

- 路線 (a) 接線＋補缺：idevice 0.1.68 自帶全 RP 棧，
  以改名相依 `idevice68` 並存引入（inventory §1/§7）。
- 協議五層（傳輸/pair-verify/SRP setup/會話加密＋
  createListener/TLS-PSK＋CDTunnel/用戶態 TCP/RSD
  RemoteXPC）皆有同作者 Rust 實作，mm-rp 只封裝會話
  語意與錯誤映射，不重寫密碼學。
- 兩後端快取粒度不同（inventory §8.5）：gw-idevice
  快取 (adapter, handshake)；gw-limd 快取 tunnel/rsd
  但 pair-verify client 每次新建——mm-rp 以
  `RpSession::establish`（全序建立）＋generation 失效
  同時服務兩形。

## 3. 差異清單（D-M7-*）

- **D-M7-1（gw-limd RP 驗證條件，顯名偏離）**：Swift
  limd 在 RP 模式下 verifyInitialized 恆敗（inventory
  §8.1 的死結：cachedUDID 永不被 RP start 設置）。
  移植改以「isInitialized && rpIdentity 已載入」作為
  RP 模式的驗證條件；lockdown 模式維持原條件。理由：
  照抄即整個 limd RP 面為死碼，與 M7 驗收矛盾；偏離
  只影響「何時回 notInitialized」，不改任何線上行為。
- **D-M7-2（雙 idevice 版本並存）**：workspace 同時有
  idevice 0.1.29（gw-idevice lockdown 路＋legacy 根
  crate）與改名相依 idevice68＝0.1.68（mm-rp＋兩
  gateway 的 RP 路）。兩版型別不互通；RP 會話的型別
  全數封裝在 mm-rp 內，gateway 只見 mm-rp 的自有型別
  與（gw-idevice 服務客戶端所需的）idevice68 服務型別
  ——後者僅出現在 gw-idevice 的 RP 實作檔內。
- **D-M7-3（RSD 目錄解析實作差）**：RPK 的服務目錄以
  啟發式位元組掃描解析（inventory §2.7）；移植採
  idevice68 `RsdHandshake` 的正規 XPC 解碼。線上行為
  等價（同一 RemoteXPC 握手），目錄內容更完整；
  RPK 的六級模糊埠解析屬行為規格，於 mm-rp 複刻。
- **D-M7-4（limd AFC 查詢無 RP 路）**：afcListDirectory／
  afcReadFile／afcGetFileInfo 在 Swift limd 無 RP 分支，
  移植保留 lockdown 路（RP 模式下回連線層錯誤），
  不自創 RSD 版查詢（inventory §8.2）。
- **D-M7-5（FFI 面不新增）**：見 §1。RP 經既有導出面
  生效；若殼層段（M9 前）發現缺口，另行補登。
- **D-M7-6（RP 錯誤 reason 的碼值來源）**：Swift limd
  的 RP reason 多嵌 RPPairingKit 錯誤碼（`code <n>`，
  其列舉 −1…−13）；idevice68 的錯誤分類不同（無 RPK
  碼）。移植保留 Swift 的 reason **模板**，碼位改放
  idevice68 錯誤的 Display 文字——真機日誌比對時須知
  此差。gw-idevice 路的 reason 模板本以 FFI 訊息字串
  為主（`error: (<msg>)` 形），直接沿用。
- **D-M7-7（withRPClient 的每次新建只在建立內部）**：
  inventory 表 C 為 withRPClient 設了獨立封裝；實作上
  limd 唯一呼叫點是隧道建立，`RpSession::establish` 內部
  每次都新建 RemotePairingClient 並於隧道建成後 drop，
  語意等價，不另設 API。
- **D-M7-8（gw-idevice 重試與最終錯誤映射的落點）**：
  Swift `performWithService` 的重試包住 connect＋操作
  本體；最終映射（invalidPairingFile "Service connection
  failed…"／serviceError "Failed to connect to…"）在移植
  的落點為：操作本體的錯誤沿用各操作自己的 Swift 模板
  （其 connect 失敗本就作 "Failed to connect to <服務>,
  error: (…)"），重建階段的建立失敗走 rp_session 的映射
  （pairing 類→invalidPairingFile "Handshake failed:
  <msg>"、其餘→connectionFailed）。重試恰一次、invalidate
  語意不變。
- **D-M7-9（除錯串流不用 crate 的 DebugProxyClient）**：
  `DebugProxyClient<R>` 是八服務中唯一的泛型客戶端；其
  `RsdService` 的 RPIT future 被 gateway 重試閉包持有時
  觸發 rustc 的 "implementation of RsdService is not
  general enough"（Box 物件生命週期被泛化；探針最小重現
  已確認，包裝型別亦不能解）。改走：mm-rp
  `open_debug_stream`（服務名取自 crate 自身的
  `rsd_service_name`）＋RpStream::rsd_checkin＋移植既有
  `RspClient`（M5，為此將 RspClient 泛型化於串流型別、
  預設型參保持 0.1.29 呼叫點不變）。線上行為與型別客戶端
  等價（同 checkin、同 RSP 封包/校驗/ack 規則）。
- **D-M7-10（limd 服務名來源與 misagent 分歧）**：gw-limd
  的 RSD 服務名除 misagent 外取 idevice 棧的
  `rsd_service_name` 值（目錄精確形）；misagent 照 Swift
  limd 的 DeviceService 字面 `com.apple.mobile.
  MCInstall.shim.remote`——crate 自身作 `com.apple.
  misagent.shim.remote`，兩名分歧待真機驗（目錄實名以
  裝置廣告為準；gw-idevice 路由 crate 內部查名、無模糊，
  若真機目錄只有 MCInstall 形，gw-idevice 的 misagent
  RSD 連接會 ServiceNotFound，列入真機待驗）。
- **D-M7-11（idevice68 的來源是 git tag 而非 crates.io）**
  ：cargo 不允許同一來源兩份 semver 相容版本（0.1.29 與
  0.1.68 同屬 0.1 線）；故 0.1.29 側改精確釘 `=0.1.29`、
  idevice68 走作者倉庫 git tag `v0.1.68`（已驗其
  `idevice/src` 與 crates.io 0.1.68 發行源碼逐字相同，
  補盤審計證據直接適用）。D-M7-2 的並存設計不變。

## 4. 驗收對照（DESIGN §4 M7）

- cargo test（host 側）：
  - mm-rp 7 測：RSD 串流分幀 fixtures（BE 長度＋XML、
    duplex 往返、超限拒收、checkin 字典形狀）＋埠解析
    六級行為（精確／三後綴／前綴／子字串／無）。
  - mm-gateway mock_lockdown 25 測全過：M3 時代兩格
    「RP 操作 unsupported」已改寫為 M7 語意（真實 RP
    配對檔 start 成功、無 endpoint→deviceEndpointIp
    NotAvailable、有 endpoint 無服務→connectionFailed
    "Failed to establish RemotePairing tunnel"；心跳
    RP 路同驗 endpoint 關）。
  - mm-core core_lifecycle 35 測全過，含新增
    `is_ready_rp_mode_end_to_end`：真實 RP 配對檔經
    start 載入→型別 RpPairing、埠表預設 49152、
    service_port()=49152、第 7 關探 127.0.0.1:49152、
    iOS 26.4 下第 4 關 ipsec 條款跳過（RP 分支）。
  - 埠表行為：GatewayCore 既有測試（M2）＋上項預設值
    斷言；set_params 的 RP 埠跨後端攜帶為 M4 既有接線，
    本段未動。
- macOS CI：全 workspace check＋新 crates 測試＋雙
  slice 驗包（見回報；本機 Linux 無法鏈 limd 的測試
  二進位，limd 段以 `cargo check --features limd` 本機
  驗證、CI 為權威）。
- 真機（留待整合場）：rppairing 配對檔 isReady 綠、
  fetchUDID 成功、心跳存活 10 分鐘、RP 模式安裝一
  App；另加：兩後端 RP 同場對照（limd 對照基準是行為
  規格——Swift limd RP 為死碼，D-M7-1）、misagent RSD
  目錄實名（D-M7-10）、debugApp/debugProcess RP 路、
  personalized DDI RSD 路。
