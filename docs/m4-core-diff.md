# M4 逐行對讀差異清單：core 生命週期＋isReady 九關＋心跳＋狀態匯流排

日期：2026-11-18。規格來源：`yongshuai1013/minimuxer` @714aafa3 之
`Sources/MinimuxerImpl.swift`（533 行全讀）、`Sources/Services/HeartbeatService.swift`
（714aafa3 含我方保活修法：lifecycle 接線＋pacing；複刻對象是修法後行為）、
`Sources/MinimuxerApi.swift`（facade `Minimuxer.set`／`createInstance`／
MinimuxerAPI extension 預設值）、`Common/NetworkUtils.swift`、
`DeviceGateway/DeviceGatewayAPI.swift`（performHeartbeat／isDDIMounted 宣告）。
範圍：DESIGN §4 M4——子系統 1、2、3。安裝/DDI 掛載本體屬 M5；RP 模式
心跳屬 M7。

## 1. MinimuxerImpl → `mm-core::MinimuxerCore`

| Swift | Rust 對應 | 備註 |
|---|---|---|
| State actor：`status`（started/inprogress/stopped）、`mountTask`、`lastDocsPath`、`preferredProtocol` | `CoreState`（Mutex）：`status`、`mount_task: Option<JoinHandle>`、`last_docs_path`、`preferred_protocol` | mountTask 本體（Mounter）屬 M5；M4 保留槽位與 stop 的取消/等待語意，`mount_ddi` 經 runWithChecks 回 gateway 的 unsupportedOperation（見 D-M4-4） |
| `start`：connectionMode＝.notConfigured → 抛 `connectionModeNotConfigured`（在 network.start 之前） | 同順序 | reason 字串見 §2 |
| `network.start()` | `PollingObserver::start(Handle::current())`（回傳值同 Swift 被忽略） | NWPathMonitor→輪詢觀察器是 M2 既定替代（DESIGN §5.1） |
| state 標 `.inprogress`＋記 lastDocsPath/preferredProtocol → `runWithChecks("while starting gateway", catchAll: .invalidPairing) { gateway.start }` | 同 | 失敗時 status 留在 .inprogress（Swift 無回滾，逐字複刻） |
| `gateway.requiresUsbmuxd` → `retargetUsbmuxdAddr()`＋`restartMuxerServer()` | M3 的 `retarget_usbmuxd_address()`＋`start_proxy_for_gateway()`（mm-core 既有） | restartMuxerServer **不在** runWithChecks 內（Swift 原碼如此）：其 MinimuxerError（pairingNotLoaded／invalidPairing／connect）直抛 |
| 標 `.started` → `heartbeat.start()` | 同 | |
| `stop`：先 `heartbeat.stop()` → state 標 .inprogress、取走 mountTask 並 cancel、await 其 result → requiresUsbmuxd 才 `proxyServer.stop()` → `runWithChecks("while stopping gateway", catchAll: .close) { gateway.stop }` → 標 .stopped | 同順序 | gateway.stop 失敗時 status 留 .inprogress（Swift 同） |
| `restartWith`：lastDocsPath 缺 → `.mount(protocol: gateway.pairingFileType, reason: "start() should be invoked before requesting <op>. cause: lastDocsPath is nil")`；否則 stop＋start | 同 | op 字串：restart／reinitializePairingData |
| `restart`：gateway.pairingFileData 非 UTF-8/缺 → `.invalidPairing(protocol:, reason: "No existing pairing file found in gateway during restart")`；restartWith 後 `network.refreshEndpoint()` | 同 | |
| `reinitializePairingData`：restartWith（新配對檔） | 同（不額外 refreshEndpoint——Swift 只有 restart 有） | |
| `runWithChecks`：DeviceGatewayError → `asMinimuxerError(protocol: activeProtocol, catchAll:)`；MinimuxerError 原樣；其餘 → `catchAll("\(error)")` | `run_with_checks`＋`map_gateway_error`：DeviceGatewayError domain → `as_minimuxer_error`（M1 既有）；MinimuxerError domain → 原樣；其餘 domain → `catchAll(info.description())` | 見 D-M4-5 |
| `setLogging`：MinimuxerLogging＋gateway.setLogging | `mm_common::logging::set_logging_enabled`＋gateway | 同 |
| `deviceProbeTimeout`／`setDeviceProbeTimeout` | 直達 mm-net manager（M2 既有） | 同 |
| `testDeviceConnection(ifaddr:)`（無 timeout 版）＝timeout 取 `deviceProbeTimeout`；目標埠＝`gateway.servicePort` | `test_device_connection(ip, None)` → manager 的 device_probe_timeout | 預設 250ms |
| `bindConnectionConfig`：manager.bind → `network.refreshEndpoint()` | 同 | |
| `getConnectionMode` | manager.get_preferred_connection_mode | 同 |
| `fetchUDID`：runWithChecks(catchAll: .fetchUDID) | 同 | |
| `isDDIMounted`：runWithChecks(catchAll: .mount) | 同；gateway 側 M4 補 trait 方法（預設 unsupported，M5 實作） | |
| `debugApp`/`attachDebugger` 的 `ensureDDIMounted` 前置 | **M5**（debug 面屬 M5） | M4 不落 |
| DeviceProvider（NSLock 持有 gateway、執行期換入） | `CoreGatewayProvider` trait（mm-core 定義）：`current_gateway`（沿用 mm-gateway 的 GatewayProvider）＋`set_backend`＋`backend`；mm-ffi 以 GatewayHub 適配（current＝ensure_gateway，恒有實例，同 Swift createInstance） | |

## 2. isReady 九關 → `MinimuxerCore::is_ready`（逐關，reason 逐字）

1. `!isPairingFileLoaded` → `.pairingNotLoaded("No valid pairing file has been loaded")`
2. `status != .started` → `.notStarted("Minimuxer has not been started")`
3. `withNetworkCheck && !network.isWifiSatisfied` → `.noConnection("No wifi interface satisfied")`。
   Swift 只認 wifi（wired/usb/bridge 在原碼被註解），逐字複刻；Rust 以
   mm-net `is_wifi_satisfied(新鮮 snapshot)` 對應（M2 既定近似，DESIGN §5.1）。
4. connectionMode 分流：
   - `.notConfigured` → `connectionModeNotConfigured("Connection mode not configured. Supported modes: [localVPN, remoteServer]")`（Swift `"\(modes)"` 陣列插值；列舉 case 名以逗號分隔——Swift 反射格式的移植近似，App 只比對錯誤類別）
   - `.localVPN`：無 utun → `.noVPN("No utun interface detected — LocalDevVPN is not connected")`；
     配對型別 ≠.rppairing **且** 無 ipsec **且** iOS ≥ 26.4 →
     `.invalidVPN("utun is present but no ipsec/IKEv2 interface found — LocalDevVPN may not support the lockdown protocol on iOS 26.4+")`。
     Swift 的 `#available(iOS 26.4, *)` 在 if 內層（先算 `!isIKEv2IPSecAvailable` 再判版本）；Rust 以 `OsVersionProvider`（Apple 目標 sysctl `kern.osproductversion`、其餘平台恆 false）實現，iOS 15 目標上恆不觸發（DESIGN 風險 10）。
   - `.remoteServer`：不查介面。
5. `pairingFileType == .unknown` → `.pairingNotLoaded("No valid pairing file has been loaded in Minimuxer")`（與第 1 關 reason 不同，逐字保留）。
6. `endpoint.ip()` 失敗 → `.localVPN`：`.noDevice("VPN tunnel iface is up but tunnel peer IP is not yet reachable — VPN may not be routing device traffic correctly. Cause: <error>")`；`.remoteServer`：`.noDevice("Remote endpoint IP is not configured or reachable. Cause: <error>")`；`.notConfigured`：回第 4 關的 notConfigured 錯誤（Swift 同，實際不可達）。`<error>`＝Swift `localizedDescription`；Rust 以 InternalError 的移植 `description()` 代入（D-M4-6）。
7. `testDeviceConnection(ifaddr: deviceIp)`（timeout＝deviceProbeTimeout）不通 →
   `.localVPN`：`.invalidVPN("VPN tunnel iface is up and tunnel peer IP <ip> is known, but TCP port poll failed — device may be unreachable on this interface")`；
   `.remoteServer`：`.notReachable("Remote endpoint <ip> is configured, but TCP port poll failed — target device is unreachable")`。
8. `fetchUDID()` 失敗 → 錯誤原樣（已經 runWithChecks 映射的 MinimuxerError）。
9. `gateway.requiresUsbmuxd && !proxyServer.isListening` → `.muxerNotListening("Usbmuxd fake server is not listening")`。
10. `withDDIMountCheck` → `isDDIMounted()` 的 Bool 為 success 值、錯誤原樣；否則 `.success(true)`。

statusPublisher：`network.onNetworkChanged` → `isReady()`（預設參數：
withNetworkCheck=true、withDDIMountCheck=false）→ `statusSubject.send(result)`。
PassthroughSubject 無重播。Rust：core 在建構時向 NetStack 註冊 change
handler（Weak 自引用）→ is_ready → 推給 `subscribe_status` 訂閱者；
mm-ffi 把訂閱接到 M1 的 EventSink `on_status`（帶序號）。見 D-M4-3。

## 3. HeartbeatService → `mm-core::HeartbeatService`（714aafa3 修法後行為）

- MutableState actor：`running`、`taskActive`；`tryStart`：taskActive 時
  只把 running 設 true 並回 false（不另起任務——含「stop 進行中又 start
  會把 running 拉回 true」的 Swift 原語意，逐字複刻並註記）；否則
  running=true、taskActive=true、回 true。任務結束時 `terminate`
  （兩旗標清零）＋`lastBeatSuccessful=false`。
- `stop`：running=false、`lastBeatSuccessful=false`、取消並等待任務。
  在飛的一拍不打斷（Swift 註解明言；取消只作用於 sleep）——Rust 以
  stop Notify 喚醒所有 sleep 段、perform_heartbeat 的 await 不包 select，
  stop 等待 JoinHandle，界限同 Swift（該拍的 Marco 逾時）。
- 主迴圈：
  1. `gateway.requiresUsbmuxd` 時先以 **1 秒** cadence 輪詢
     `proxyServer.isListening`，每圈查 running，未 listening 不進主迴圈。
  2. `currentInterval` 初始＝`heartbeatInitialIntervalSeconds`（5）。
  3. `endpoint.ip()` 取不到 → log（去重）、lastBeatSuccessful=false、
     睡 **不可達退避**（見 D-M4-1）後 continue。
  4. `NetworkUtils.testTCP(ip, gateway.servicePort)` 不通 → 同上。
     注意：此探測用 NetworkUtils 的預設逾時＝常數
     `defaultTCPProbeTimeoutMs`（250），**不是** deviceProbeTimeout
     （後者只走 MinimuxerImpl.testDeviceConnection 路徑）。
  5. `gateway.performHeartbeat(interval: clamp(currentInterval))`：
     成功 → currentInterval＝回傳值（原值，下次用時才鉗）、
     lastBeatSuccessful=true、清 log 去重；
     失敗 → log（去重）、lastBeatSuccessful=false、
     currentInterval 重置 5。
  6. 兩路徑皆 `pacingSleep`：`max(clamp(interval) × 0.75, 4.0)` 秒。
- `clampInterval`：鉗 `heartbeatMin/MaxIntervalSeconds`＝[5, 60]。
- log 去重：`lastErrorDescription` 只在迴圈內存取（Swift 是實例變數
  但僅迴圈使用），Rust 收進迴圈區域狀態。
- lastBeatSuccessful：不對外發布、不餵 isReady（九關不含它）——
  INVENTORY 2.3 的定讞；Rust 保留同名查詢供 FFI/測試。

### gateway.performHeartbeat（M4 補進 mm-gateway trait；兩後端 lockdown 路）

- 共同：`verifyInitialized` 先行；rppairing → `unsupportedOperation`
  （RP 心跳走 RSD stream，M7）；回傳 `newInterval > 0 ? newInterval : interval`
  的 fallback 在 gateway 層（兩後端 Swift 的 async 包裝皆有此行）。
- gw-idevice：`HeartbeatClient::connect`（idevice crate；
  lockdownd session＋StartService＋連線）失敗 →
  `.serviceError("Failed to connect to heartbeat, error: (<msg>)")`
  （performWithEitherService 的 connect 失敗格式，serviceName="heartbeat"）；
  `get_marco(clamp 已由呼叫端處理的 interval)` 失敗 →
  `.serviceError("Heartbeat receive failed, error: (<msg>)")`；
  `send_polo` 失敗 → `.serviceError("Heartbeat send failed, error: (<msg>)")`。
- gw-limd：withDevice → `HeartbeatClient::new`（rusty_libimobiledevice，
  heartbeat_client_start_service）失敗 →
  `.serviceError("Failed to create client for 'com.apple.mobile.heartbeat': code <n>")`
  （withService 的 create 失敗格式；HeartbeatError 反查碼表比照 M3 既有做法）；
  送 `{"Command": "Pico"}` plist，`heartbeat_send` 失敗 →
  `.serviceError("heartbeat_send failed with code <n>")`；
  `heartbeat_receive_with_timeout(5000ms)`：有回應且含 `Interval`
  （uint，或 int——RSD 分支有 int 分支、lockdown 分支只讀 uint；
  Rust 值型別統一以 as_unsigned 讀取，負值視同缺值）→ newInterval；
  無 Interval 且 receive 成功 → newInterval＝interval；
  receive 失敗 → `.serviceError("heartbeat_receive_with_timeout failed with code <n>")`。
  注意 Swift lockdown 分支的順序：先查 respPlist 的 Interval（有即
  return），再查 rRes——receive 回錯誤碼但 plist 非空且含 Interval 時
  Swift 不抛錯；wrapper 的 receive 把「錯誤碼≠Success」直接變 Err、
  plist 不可達，此格以 wrapper 語意為準（D-M4-7）。

## 4. facade `Minimuxer.set(MinimuxerParams)` → `MinimuxerCore::set_params`

Swift（Minimuxer class，靜態存值 currentBackend/currentRemotePairingPort/
currentDeviceProbeTimeout）：

- backend 有值且 ≠ 現行 → 建新 gateway、把 `currentRemotePairingPort`
  設進新 gateway 的 .rppairing 埠、DeviceProvider 原子換入。
- remotePairingPort 有值且 ≠ 存值 → 更新存值＋對**現行** gateway
  `setPort(port, for: .rppairing)`。
- deviceProbeTimeout 有值且 ≠ 存值 → `core.setDeviceProbeTimeout`。

Rust：存值收進 core（`current_rp_port`，初始＝常數 49152）；
backend 切換委派 provider.set_backend（mm-ffi 的 hub：M3 既有語意是
「新實例＋把舊 gateway 的 RP 埠帶入」——與 Swift「把 facade 存值設入」
在「埠只經 set_params 改」的常態下等值；若埠曾經 gateway 層直接改
（如 resolveServicePort 回寫），兩者結果不同，M3 既定選擇不回頭，
見 D-M4-2）；切換後 core 再以存值 setPort 一次（常態下 no-op）。
deviceProbeTimeout 存值即 manager 現值（core 直查直設）。

## 顯名偏離（DESIGN D5 的例外登記）

- **D-M4-1（DESIGN §4 M4 預定偏離，本案唯一預定項）**：心跳的
  「不可達路徑」（endpoint 取不到／TCP 探測不通）以 d044c752 的
  退避曲線 **5→10→20→40→60 秒（加倍、上限 60）** 取代 Swift 固定
  5 秒；進入可達（探測通過）即重置回 5。成功路徑與 perform_heartbeat
  失敗路徑的 pacing（75%／下限 4s）不變——不改變成功路徑節奏。
  舊 Rust 長連線模型的逾時鉗 [20,120] 不採用（Swift 每拍新連線
  模型為規格，DESIGN 風險 8；真機 A/B 留真機場）。
- **D-M4-2**：backend 切換時 RP 埠的帶入來源：hub（M3）以舊 gateway
  現值帶入，Swift facade 以 facade 存值帶入；core.set_params 在
  hub 切換後再以存值覆寫，常態路徑（埠只經 set_params 改）兩者一致。
- **D-M4-3**：狀態匯流排的 FFI 形狀：EventSink.on_status 只有
  （success, domain, code, reason），無法攜帶 `.success(false)` 的
  Bool 值（僅 withDDIMountCheck 路徑可達、App 未使用、網路重探用
  預設參數不會產生）；publisher 推播 success 時一律視為 true。
  `mm_core_is_ready` 的直接查詢面另有專用 outcome 結構攜帶 Bool，
  不受此限。
- **D-M4-4**：mountDDI 本體（Mounter）與 debug 前置
  ensureDDIMounted 屬 M5；M4 的 `mount_ddi` 保留 Swift 的 mountTask
  管理（取消舊任務、等待、換入新任務、記 lastDocsPath），任務本體
  回 gateway unsupportedOperation 經 runWithChecks 映射為
  `.mount(protocol:, reason: "mountDDI")`；`is_ddi_mounted` 的
  gateway trait 預設同理（M5 實作後自動生效）。
- **D-M4-5**：runWithChecks 的泛型 catch 字串：Swift 用 `"\(error)"`
  （反射式列舉 dump）；Rust 用 MmErrorInfo 的移植 `description()`
  （M1 已逐字對照 Swift 的 description 模板）。App 可見的錯誤
  **類別**（invalidPairing 等）與描述文案一致，反射格式本身不移植。
- **D-M4-6**：isReady 第 6 關的 `Cause:` 後綴：Swift 用
  `error.localizedDescription`；Rust 用 InternalError 移植
  description()。文案前綴逐字，後綴為移植描述。
- **D-M4-7**：gw-limd 心跳 receive 的「錯誤碼＋非空 plist」格：
  Swift 先讀 plist 再判錯誤碼；rusty_libimobiledevice wrapper 在
  錯誤碼≠Success 時不交回 plist，此格以 wrapper 語意為準（receive
  失敗即抛 serviceError）。真機上 heartbeat_receive_with_timeout
  逾時回 -6（Timeout）且無 plist 為常態路徑，兩者一致。

## 真機待驗（本段無真機，留主代理安排）

- 狀態點在隧道開／關／重開三態與 Swift 版同色同時序。
- 裝置註冊流程（isReady 第 8 關 fetchUDID 經 core）通過。
- BrokenPipe／early eof 場景 A/B：同裝置同隧道，Swift 版 vs Rust 版
  各 30 分鐘計斷線次數（DESIGN 風險 8 的驗收，本段只備好儀器：
  心跳 log 去重行＋lastBeatSuccessful 查詢）。
- E1 判準（隧道切換 ≤2 秒 endpoint 更新）仍掛 M2 遺留，與本段
  status 重探時序同場驗。
