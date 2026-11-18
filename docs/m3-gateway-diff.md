# M3 逐行對讀差異清單：gateway lockdown 基本鏈＋usbmuxd 代理

日期：2026-11-18。規格來源：`yongshuai1013/minimuxer` @714aafa3 之
`DeviceGateway/DeviceGatewayAPI.swift`、`BaseDeviceGateway.swift`、
`DeviceGatewayError.swift`、`idevice/IdeviceGateway.swift`、
`libimobiledevice/LibimobiledeviceGateway.swift`、
`Sources/Services/UsbmuxdProxyServer.swift`、`Sources/Types/RawPacket.swift`、
`Sources/MinimuxerImpl.swift`（start/stop/restartMuxerServer 段）、
`Sources/MinimuxerApi.swift`（GatewayBackend／Minimuxer.set）、
`Sources/Services/NetworkObserverService.swift`（refreshEndpoint 段）。
範圍：DESIGN §4 M3——子系統 11 之 lockdown 子集＋子系統 7。RP 路徑只對讀
「start 接受度」與分流點，連接實作屬 M7。

## 1. BaseDeviceGateway → `mm-gateway::GatewayCore`

| Swift | Rust 對應 | 備註 |
|---|---|---|
| `pairingFileType`（預設 `.unknown`） | `pairing_file_type` | 同 |
| `pairingFileData`（didSet 衍生 dict，`try?` 失敗即 nil） | `set_pairing_file_data` 同步衍生 `pairing_data_dict` | plist 解析失敗 → dict 為 None |
| `deviceEndpointIp` | `device_endpoint_ip` | 同 |
| `isInitialized` | `is_initialized` | 同 |
| `protocolPorts` 表＋`getPort`（缺值回 `defaultPort`） | `protocol_ports`＋`get_port` | `.unknown` 的 defaultPort 依 `#available(iOS 17)`；Rust 以建構參數 `ios17_available` 承接，預設 `false`（與 mm-net TablePortSource 的 M2 既定選擇一致；目標裝置 iOS 15） |
| `setPort`：同值 no-op；異值寫入＋`invalidateConnection()` | 同（generation +1） | Rust 以 generation 計數器實現 invalidate：後端重用快取連線前比對 generation，落差即丟棄（惰性失效，與 Swift 子類別即時 free 語意等價於「下次不用舊連線」） |
| `setDeviceEndpointIp`：同值 no-op（含 log）；異值寫入＋invalidate | 同 | 同上 |
| `cleanup()`：initialized=false、data=nil、type=.unknown、invalidate；**不清 protocolPorts** | 同 | 埠表跨 stop/start 存活，逐字複刻 |
| `stop()` = cleanup | 同 | 兩後端 Swift 皆為 cleanup 後再 super.stop()（=再 cleanup 一次，冪等） |

## 2. IdeviceGateway（start 與服務連接段）

- `requiresUsbmuxd = false`；`shared` 單例存在，但 facade 的 `makeGateway`
  每次切換皆建新實例——Rust hub 同此（切換即新實例，舊實例釋放；見 §6 D-8）。
- `syncStart`：先 `cleanup()` → `PairingFileParser.parse`（**PairingError
  原樣上抛**，不包成 DeviceGatewayError）→ set data/type →
  lockdown：`idevice_pairing_file_from_bytes` 失敗 →
  `invalidPairingFile("idevice_pairing_file_from_bytes failed")`；
  rppairing：`rp_pairing_file_from_bytes` 失敗 →
  `invalidPairingFile("rp_pairing_file_from_bytes failed")` →
  `isInitialized = true`。
- `verifyInitialized`：僅查 `isInitialized`（與 limd 版不同，見 §3）。
- 服務連接四路：
  - `performWithService`（RP）：`ensureRPConnection`（adapter+handshake
    快取；endpoint 缺 → `deviceEndpointIpNotAvailable`；pairingFile 缺 →
    `notInitialized("pairingFile is nil")`）→ connect 失敗 invalidate 後
    **重試一次**（新隧道）；二敗依 `isPairingError` 分類
    invalidPairingFile／serviceError。**M7 實作，M3 不落。**
  - `performWithUsbmuxdService`：讀 env `USBMUXD_SOCKET_ADDRESS`
    （含 ":" → TCP addr；否則 unix path）→ 預設 addr → 連 usbmuxd →
    get_devices 取第一台 → UsbmuxdProvider。**全檔無呼叫點**（定義未用）；
    見 D-1。
  - `performWithTcpService`（lockdown）：endpoint 缺 →
    `deviceEndpointIpNotAvailable`（無 reason）；pairingFileData 缺 →
    `connectionFailed("pairingFileData is nil")`；以
    **常數 `lockdowndPort`（62078）**建 TcpProvider（注意：不是
    `getPort(for: .lockdown)`，見 D-4）→ connect 失敗 →
    `serviceError("Failed to connect to <service>, error: (<msg>)")`；
    client 為 nil → `noConnection`。
  - `performWithEitherService`：依 `pairingFileType` 分流
    （rppairing→RP 路、其餘→TCP 路）。所有 lockdown 操作皆經此。
- `syncGetLockdownValue`：verify → either 路連 lockdownd →
  **lockdown 模式先 `lockdownd_start_session`**（pairing 分類錯誤 →
  `invalidPairingFile("Lockdown session failed (pairing invalid): <msg>")`，
  其餘 → `connectionFailed("Lockdown session failed: <msg>")`）→
  `lockdownd_get_value`（pairing 分類 → invalidPairingFile，否則
  serviceError，reason 皆 `"Failed to get lockdown value for key <k>, error: (<msg>)"`）
  → 值須為字串，否則
  `serviceError("Lockdown value for key '<k>' could not be decoded as string")`；
  值節點缺 → `serviceError("Lockdown value for key '<k>' is missing")`。
- `syncFetchUDID` = getLockdownValue("UniqueDeviceID")，空字串 →
  `serviceError("UniqueDeviceID not found on device")`。
- `isPairingError` 分類：FFI code 18（InvalidHostID）／103（RemotePairing），
  或 message（小寫）含 `invalidconf`／`pairing`／`handshake`／
  `bad_certificate`／`certificate_unknown`。Rust 對 `idevice::IdeviceError`
  同表判定（`InvalidHostID` 變體直接命中，其餘以 Display 字串比對）。
- `invalidateConnection`：僅 free RP adapter/handshake（lockdown 路無狀態）。
- `setLogging`：FFI logger 級別（release 版 enabled→1/否→0）。Rust 側
  idevice crate 走 `log` facade，無 FFI logger 可設；M3 僅記錄於 core
  旗標（見 D-9）。

## 3. LibimobiledeviceGateway（start 與服務連接段）

- `requiresUsbmuxd = true`；init 做 OpenSSL 初始化（Rust 側由
  rusty_libimobiledevice／其相依處理，無對應動作）。
- `syncStart`：cleanup → parse（PairingError 原樣上抛）→ set data/type →
  rppairing：`rppairing_identity_from_plist` 失敗 →
  `invalidPairingFile("rppairing_identity_from_plist failed with code <n>")`；
  lockdown：`cachedUDID = lockdown.udid`，空 →
  `invalidPairingFile("Missing UDID in pairing file")` → setInitialized(true)。
- `verifyInitialized`：**`isInitialized && cachedUDID != nil`**（較 idevice
  後端嚴格，逐後端複刻）。
- 服務連接（lockdown）：`withDevice`（`idevice_new_with_options`，
  LOOKUP_USBMUX｜LOOKUP_NETWORK，依 cachedUDID；失敗 →
  `connectionFailed("idevice_new_with_options failed with code <n>")`）→
  `withLockdown`（`lockdownd_client_new_with_handshake`，失敗 →
  `connectionFailed("lockdownd_client_new_with_handshake failed with code <n>")`）→
  `withService`（`lockdownd_start_service` 失敗 →
  `serviceError("Failed to start lockdown service '<name>': code <n>")`；
  建 client 失敗 → `serviceError("Failed to create client for '<name>': code <n>")`）。
  錯誤 reason 用 **C 錯誤碼數字**，Rust 以 rusty_libimobiledevice 的
  `LockdowndError`/`IdeviceError` 變體反查 i32 表還原。
- `syncGetLockdownValue`（lockdown）：withLockdown →
  `lockdownd_get_value(client, nil, key)` 失敗 →
  `serviceError("Failed to get lockdown value for key '<k>': code <n>")`；
  字串指標 nil →
  `serviceError("Lockdown value for key '<k>' could not be decoded as string")`。
  （rppairing 走 withRSDService 的 GetValue plist，M7。）
- `syncFetchUDID`：verify → 試 live 查詢；成功且非空 → 更新 cachedUDID
  並回傳；**失敗或空 → 回退 cachedUDID**；無 cached →
  `serviceError("UniqueDeviceID not found on device")`。（與 idevice 後端
  「live 失敗即抛」不同，逐後端複刻。）
- `invalidateConnection` = cleanupRPTunnel（僅 RP 快取；lockdown 路無狀態）。
- `setLogging`：`idevice_set_debug_level`＋`rppairing_set_debug_level`；
  Rust 對應 `rusty_libimobiledevice::idevice::set_debug`（RP 半部無對應，
  M7 再議）。

## 4. UsbmuxdProxyServer＋RawPacket（子系統 7）→ `mm-core`

- 綁 `127.0.0.1:27015`（常數 usbmuxdHost/usbmuxdPort），
  allowLocalEndpointReuse；多連線並行、逐連線依序讀包回應。
- 讀包：先讀 4-byte LE size；合法範圍 `16 ≤ size ≤ 4095`
  （usbmuxHeaderLen／usbmuxMaxPacketBufferLength），越界即斷線；再讀
  size−4。RawPacket：16-byte header（size/version/message/tag 皆 LE u32）
  ＋XML plist body；解不出（長度不足／plist 非 dict）→ **不回應、續讀**
  （`guard let packet else return`＋defer 重讀）。
- 回應包：version=1、message=8、tag 回顯請求 tag。
- `handlePacket` 語意（**錯誤一律被 processPacketData 的 `catch {}` 吞掉：
  不回應、不斷線、續讀**——INVENTORY 2.7「抛錯斷線」與碼不符，以碼為準，
  見 D-3）：
  - MessageType 缺 → throw `MinimuxerError.connect("Malformed usbmuxd packet: missing MessageType field")`（吞）。
  - `ListDevices`：無 attach peer → `{"DeviceList": []}`；有 peer 但無
    UDID → throw `invalidPairing(lockdown, "No device UDID available for ListDevices response")`（吞）；
    否則回一台：`DeviceID=1`、`Properties{ConnectionType:"Network",
    DeviceID:1, EscapedFullServiceName:"<udid>._apple-mobdev2._tcp.local",
    InterfaceIndex:0, NetworkAddress: convertIp(peer), SerialNumber: udid}`。
    注意 device 條目**無** MessageType 鍵（舊 Rust 版有 "Attached"，已改）。
  - `Listen` → `{"MessageType":"Result","Number":0}`。
  - `ReadBUID` → `{"BUID": gateway.pairingDataDict["SystemBUID"] ?? "00000000-0000-0000-0000-000000000000"}`。
  - `ReadPairRecord` → gateway.pairingFileData 原始 bytes；缺 →
    throw `pairingNotLoaded("No pairing file data available for ReadPairRecord")`（吞）。
  - 未知 type → throw `connect("Unsupported usbmuxd message type: <t>")`（吞）。
- `convertIp`：152-byte sockaddr_storage。v4：sockaddr_in 鋪滿前 16 byte
  （sin_len=16、family=2、port=0、addr@4..8，其餘 0）。v6：sockaddr_in6
  （sin6_len=28、family=30、port/flow=0、addr@8..24、scope_id@24..28
  小端序）；scope 來源：`%iface` 後綴 → if_nametoindex；fe80: 前綴 →
  en0 → awdl0 → lo0 依序取第一個非 0。
- 生命週期：`start(udid)` 已啟動回 `false`（不抛）；綁定失敗 →
  `MinimuxerError.connect("UsbmuxdProxyServer failed to bind: <err>")`；
  `stop()` 未啟動即 no-op；`isListening`/`started` 兩旗標。
  attach/detach：`notifyDeviceAttached(tunnelPeerIp:)`/
  `notifyDeviceDetached()` 設定 currentDeviceIp（NetworkObserverService
  在 refreshEndpoint 的 localVPN/remoteServer 兩分支呼叫；mm-net 的
  `AttachListener` 已是對應接點）。
- core 編排（MinimuxerImpl）：`start` 時 gateway.requiresUsbmuxd →
  `retargetUsbmuxdAddr()`（unsetenv 後 setenv
  `USBMUXD_SOCKET_ADDRESS=127.0.0.1:27015`）＋`restartMuxerServer()`
  （stop 後 start；pairing dict 缺 →
  `pairingNotLoaded("Pairing dictionary is missing in gateway")`；
  UDID 缺 → `invalidPairing(activeProtocol, "Pairing file is missing UDID value")`）；
  `stop` 時 requiresUsbmuxd → proxy.stop()。M3 落為 mm-core 編排函數，
  facade 串接在 M4。

## 5. 後端選擇（App 可見語義）

- `GatewayBackend: String` = `libimobiledevice`／`idevice`，預設 `idevice`
  （MinimuxerApi.swift；App UserDefaults `minimuxerGatewayBackend`）。
- `Minimuxer.set(MinimuxerParams)`：backend 變更 → `makeGateway` 建新實例
  → 新 gateway 先 `setPort(currentRemotePairingPort, for: .rppairing)`
  → 換入 DeviceProvider；remotePairingPort 變更 → 對現 gateway setPort。
  Rust hub 複刻：切換建新實例並把舊 gateway 的 rppairing 埠值帶入新實例。

## 6. 顯名偏離與裁示

- **D-1**：Swift `performWithUsbmuxdService` 無呼叫點（死路徑）。Rust
  gw-idevice 實作等價 helper（`idevice::UsbmuxdProvider`，env
  `USBMUXD_SOCKET_ADDRESS` 優先、預設指本機代理），但生產路由仍依
  `performWithEitherService`（lockdown→TCP）；usbmuxd 代理資料路的實際
  行使者是 gw-limobiledevice（libusbmuxd 探索＋直連服務），與 Swift 一致。
- **D-2**：RP 模式 start：Swift 於 start 即建 RP FFI 物件；Rust M3 僅存
  解析後狀態並標 initialized，RP 連接操作回
  `unsupportedOperation`，待 M7（mm-rp）。lockdown 子集不受影響。
- **D-3**：INVENTORY 2.7「未知 MessageType 抛錯斷線」與碼不符：碼為吞錯
  續讀。Rust 從碼。
- **D-4**：gw-idevice 的 TCP 路徑埠號用常數 62078，不讀埠表 lockdown
  項（Swift 如此）；埠表的 lockdown 項僅供 mm-net 探測與
  `servicePort` 呈現（Swift 同）。
- **D-5**：舊 Rust `muxer.rs` 對 Swift 規格的偏離全數修正：DeviceID
  420→1、EscapedFullServiceName 佔位字串→`"<udid>._apple-mobdev2._tcp.local"`、
  NetworkAddress 寫死 10.7.0.1→attach 現值、InterfaceIndex 69→0、
  單連線串行→多連線並行、缺 ReadBUID→補、無 attach/detach→補、
  無 stop→補；舊版獨有的「accept 失敗 50 次 rebind 自癒」不移植
  （Swift 靠 listener state 上報，綁定失敗即錯誤）。
- **D-6**：sockaddr 位元佈局從 Swift/Darwin（sin_len=16、v6 addr@8），
  非舊 Rust convert_ip（首 byte 10、v6 addr@16）。
- **D-7**：limd 後端以 feature `limd` 隔離（rusty_libimobiledevice 為
  pregenerated extern 綁定，host 無 libimobiledevice 時 test 二進位無法
  連結；比照舊倉 CI 從不跑其 test 的現實）。mm-ffi 以 feature 轉發，
  xcframework 構建啟用；host test 不啟用。
- **D-8**：hub 切換後端時舊 gateway 實例直接釋放（Swift 的 `.shared`
  單例會留存但換入後不可達；App 可見行為一致）。
- **D-9**：`setLogging` 在 Rust 僅落 core 旗標＋（limd）`set_debug`；
  idevice crate 的 log 級別由宿主 `log` 設定決定，M4 facade 再統籌。

## 7. 錯誤映射總則

gateway 操作錯誤一律以 `mm_common::MmErrorInfo` 跨 trait 邊界：
PairingError 保其 domain（start 解析段，Swift 原樣上抛）；
DeviceGatewayError（domain `DeviceGatewayError`，8 code）由兩後端依
上表 reason 格式產生；代理編排錯誤為 MinimuxerError domain。
`idevice::IdeviceError` → 分類：`InvalidHostID` 及 Display 命中
isPairingError 字串表 → pairing 類；其餘依操作段落映射
connectionFailed／serviceError（reason 格式逐段如上）。
