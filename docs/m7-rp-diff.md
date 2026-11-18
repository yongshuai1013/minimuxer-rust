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
