# M9 全量等價對照表（INVENTORY §3 逐項審計）

日期：2026-11-18。基準：Swift `yongshuai1013/minimuxer` @714aafa3
（INVENTORY.md §2/§3 原始盤點）；實況：本倉 `rust-rewrite`
@31c36824（M8 結案樹）。INVENTORY §2 各段「Rust 對照」註記為
M1 前舊狀態，本表逐項以現況覆核、不採信舊註記。

四態定義：

- 行為與 Swift 一致，附 Rust 落點與測試證據。
- 行為有差異，已在各段差異清單以 D-* 編號裁示。
- 不移植／降級，附原始裁示出處（DESIGN／盤點）。
- 真未實作。本表結算：**缺口 0 項**（詳文末結算）。

測試證據以測試函數所在檔案計；本機 host 數字（Linux）：
mm-common 21、mm-net 45、mm-rp 16、mm-gateway 53、
mm-core 60、mm-emproxy 24、mm-ffi 25，合計 244 全綠
（2026-11-18 本段覆跑）。macOS CI 為權威（見結算節）。

## §1 Facade 與 MinimuxerAPI（INVENTORY 2.1）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| A1 | Facade.core | 已等價 | mm-core `MinimuxerCore`；FFI `mm_core_*` 全面 |
| A2 | Facade.network | 已等價 | mm-net observer/manager；FFI `mm_net_*` |
| A3 | Facade.wirelessPair | 已等價 | mm-core `wireless_pair::WirelessPairService`；FFI `mm_wireless_*` |
| A4 | Facade.emproxy | 已等價 | mm-emproxy `EmProxy`；FFI `mm_emproxy_*` |
| A5 | Facade.gateway | 已等價 | mm-ffi `gateway_hub`；FFI `mm_gateway_*` |
| A6 | Facade.set(params) | 顯名偏離 | `MinimuxerCore::set_params`＋hub 換後端；D-M4-2（RP 埠帶入來源） |
| A7 | Params.backend | 已等價 | `MmGatewayBackend`／`GatewayBackend`；core_lifecycle set_params 測 |
| A8 | Params.remotePairingPort | 顯名偏離 | 同 A6，D-M4-2 |
| A9 | Params.deviceProbeTimeout | 已等價 | core 直查直設 manager 現值（m4 diff §2） |
| A10 | core.pairingFileType | 已等價 | `MinimuxerCore::pairing_file_type`；FFI `mm_gateway_pairing_protocol` |
| A11 | core.isLoggingEnabled | 已等價 | `MinimuxerCore::is_logging_enabled`（mm-common logging 旗標） |
| A12 | core.isPairingFileLoaded | 已等價 | `is_pairing_file_loaded`；FFI `mm_core_is_pairing_file_loaded` |
| A13 | core.deviceProbeTimeout（屬性） | 已等價 | `device_probe_timeout()`；FFI `mm_net_set_probe_timeout` |
| A14 | core.statusPublisher | 顯名偏離 | 狀態匯流排→EventSink `on_status(seq,success,domain,code,reason)`；D-M4-3（`.success(false)` 的 Bool 在推播面遺失，僅 DDI 選配路可達、App 未用；直接查詢面 `mm_core_is_ready` 有專用 outcome 攜 Bool，不受限） |
| A15 | getConnectionMode | 已等價 | `get_connection_mode`；FFI `mm_net_connection_mode` |
| A16 | isReady(九關＋選配第十關) | 已等價 | `MinimuxerCore::is_ready` 逐關複刻（m4 diff §2）；DDI 語意專節見 §9 |
| A17 | describeError | 已等價 | `describe_error`（error-map 模板）；FFI `mm_describe_error`；error_map.rs 測 |
| A18 | bindConnectionConfig | 已等價 | `bind_connection_config`；FFI `mm_core_bind_connection_config` |
| A19 | setLogging | 顯名偏離 | D-9：Rust 落 core 旗標＋limd `set_debug`；idevice crate 級別由宿主 log 設定 |
| A20 | setDeviceProbeTimeout | 已等價 | `set_device_probe_timeout` |
| A21 | start | 已等價 | `MinimuxerCore::start`（分階段：network→gateway→proxy→heartbeat，m4 diff §1） |
| A22 | stop | 已等價 | `stop`（先停心跳、等 mountTask、停 proxy、gateway.stop） |
| A23 | restart | 已等價 | `restart`（存值 stop+start；App 層另以 stop+start 組合，兩路皆通） |
| A24 | reinitializePairingData | 已等價 | `reinitialize_pairing_data` |
| A25 | mountDDI | 已等價 | `mount_ddi`＋`mounter::Mounter`（M5 本體，D-M4-4 結案）；錯誤形狀 D-M5-10 |
| A26 | isDDIMounted | 已等價 | `is_ddi_mounted` |
| A27 | fetchUDID | 已等價 | `fetch_udid`（run_with_checks 映射，gate8 測） |
| A28 | testDeviceConnection | 已等價 | `test_device_connection(ifaddr, timeout)`；FFI `mm_test_tcp` |
| A29 | sendIpaAfc | 已等價 | core→gateway 轉發（§6 K21） |
| A30 | sendAppBundleAfc | 顯名偏離 | D-M5-13（走訪順序：Rust 排序、Swift 未定義；結果等價） |
| A31 | installIpa | 顯名偏離 | D-M5-2（gw-idevice 自寫 instproxy plist 協議） |
| A32 | installAppBundle | 顯名偏離 | D-M5-2 |
| A33 | removeApp | 顯名偏離 | D-M5-2 |
| A34 | wipeContainer | 顯名偏離 | D-M5-8（gw-limd reason 合併）；gw-idevice 形狀等價 |
| A35 | debugApp | 顯名偏離 | D-M5-5／D-M7-9（自寫 RspClient 取代 crate DebugProxyClient） |
| A36 | attachDebugger | 已等價 | `attach_debugger`→gateway `debug_process`；lockdown idevice 同形 unsupported（D-M5-1，見附錄 A-a1） |
| A37 | installProvisioningProfile | 已等價 | core→gateway（§6 K17） |
| A38 | removeProvisioningProfile | 已等價 | core→gateway |
| A39 | dumpProfiles | 顯名偏離 | D-M5-11（.zip 檔名 UTC 'Z' 組版與容器位元組不同；內容物集合相同） |
| A40 | afcListDirectory | 顯名偏離 | D-M7-4（gw-limd 無 RP 路，照 Swift） |
| A41 | afcReadFile | 顯名偏離 | D-M7-4 |
| A42 | afcGetFileInfo | 顯名偏離 | D-M7-4 |

## §2 就緒探測與心跳（INVENTORY 2.2／2.3）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| B1 | isReady 九關順序與 reason | 已等價 | core.rs `is_ready` 逐關（pairingNotLoaded→notStarted→wifi→mode→pairing 再查→endpoint→TCP poll→fetchUDID→muxerNotListening）；core_lifecycle 35 測含逐關與 RP 模式全程 |
| B2 | statusPublisher 觸發（path 變化→refreshEndpoint→重探） | 顯名偏離 | 輪詢觀察器（1s＋150ms settle）取代 NWPathMonitor 即時回呼；observer.rs 模組註解差異 1/6；E1 殼未啟用（DESIGN D4 條件式） |
| C1 | gateway.performHeartbeat | 已等價 | 兩後端 lockdown（M4）＋RP（M7）四路皆實作；heartbeat.rs／mock_lockdown 測 |
| C2 | HeartbeatService 生命週期與 pacing | 顯名偏離 | D-M4-1：不可達路徑以 5→10→20→40→60s 退避取代 Swift 固定 5s；成功／失敗 pacing（鉗 [5,60]×75%、下限 4s）逐字複刻；heartbeat 9 測 |
| C3 | 心跳前置（等 proxy listening 的 1s cadence） | 已等價 | mm-core heartbeat.rs（requiresUsbmuxd 時先等 proxy） |

## §3 網路觀察／連線管理／端點（INVENTORY 2.4／2.5／2.6）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| D1 | network.isWifiSatisfied | 顯名偏離 | observer.rs 近似「en0 存在＋預設路由」（DESIGN §5.1 顯名語意差）；satisfaction.rs 測 |
| D2 | network.isWiredSatisfied | 顯名偏離 | observer.rs 近似「預設路由經 Ethernet 分類介面」 |
| D3 | network.isUsbSatisfied | 已等價 | 掃描推導逐字（en*≠en0＋169.254.*） |
| D4 | network.isBridgeSatisfied | 顯名偏離 | NWPath `.other` 條款無等價而刪除，掃描條款（bridge/ap）逐字保留 |
| D5 | network.isUTunAvailable | 已等價 | 掃描 utun 前綴 |
| D6 | network.isIKEv2IPSecAvailable | 已等價 | 掃描 ipsec 前綴 |
| D7 | network.pathPublisher | 顯名豁免 | DESIGN D4 顯名降級：NWPath 無法合成、App 未使用；Rust 不發射（observer.rs 註解差異 5） |
| D8 | network.activeInterfaces | 已等價 | `scan_interfaces`→`MmInterfaceInfo`；scan.rs／types.rs 測 |
| D9 | network.start | 已等價 | 輪詢觀察器 start（冪等）；observer.rs 測 |
| D10 | network.stop | 已等價 | observer stop（冪等） |
| D11 | network.refreshEndpoint | 已等價 | observer `refresh_endpoint`：manager refresh 有變才更新 endpoint 並 notify proxy attach/detach |
| D12 | LocalInterfaceType 12 例＋Comparable＋isVPN | 已等價 | mm-net types.rs 逐表移植；types.rs 5 測 |
| D13 | LocalInterfaceInfo | 已等價 | types.rs（name/ip/ipv6/subnet/type） |
| E1 | 連線模式判定（binding getConnectionMode） | 已等價 | manager `get_preferred_connection_mode`；未綁定＝notConfigured |
| E2 | 候選 tunnel 篩選（utun＋v4 無 v6、依名排序） | 已等價 | `resolve_candidate_tunnels`；manager.rs 測 |
| E3 | 候選 peer 優先序（gateway /32→destination 保留 mask→link-layer /32）＋過濾表 | 已等價 | `resolve_candidate_peers_for`；manager.rs 測含今晨漂移 fixture（10.7.0.0/24 保留 mask） |
| E4 | 候選並行探測首勝 | 已等價 | JoinSet 首勝 abort 其餘（Swift task group 同語意） |
| E5 | tcpProbe 失敗→resolveServicePort 換埠再探 | 已等價 | `probe_with_source`；換埠勝出寫回 setPort；M9 補勝出日誌（§10） |
| E6 | override peer 採用規則（可達才用、不可達清 endpoint 不退回 derived） | 已等價 | observer refresh_endpoint 規則逐字；manager.rs override 測（M2 更正後語義） |
| E7 | binding 回寫（iface/peer/mask/reachable 七 setter） | 已等價 | manager refresh_local_vpn／refresh_remote_server 回寫全套；全同則回 false |
| E8 | remoteServer refresh（探 remoteServerIp、清 localVPN 探索態） | 已等價 | `refresh_remote_server` |
| F1 | endpoint.ip()（未初始化抛 Internal 錯） | 已等價 | mm-net endpoint.rs；endpoint.rs 3 測 |
| F2 | endpoint.update（同步 gateway.setDeviceEndpointIp） | 已等價 | endpoint＋observer 接線 |
| F3 | endpoint.clear | 已等價 | 同上 |
| F4 | endpoint.isInitialized | 已等價 | endpoint.rs |

## §4 usbmuxd 代理與 EMProxy（INVENTORY 2.7／2.8）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| G1 | UsbmuxdProxyServer start/stop/isListening/started | 已等價 | mm-core usbmuxd_proxy.rs（多連線並行）；usbmuxd_proxy 11 測 |
| G2 | ListDevices 回應（DeviceID=1、EscapedFullServiceName、NetworkAddress=attach 現值 sockaddr） | 已等價 | D-5／D-6（舊 Rust 偏離全數修正：420→1、佔位字串→實名、寫死 IP→現值、sin_len 佈局） |
| G3 | Listen 回 Result 0 | 已等價 | usbmuxd_proxy.rs |
| G4 | ReadBUID（缺值回全零 UUID） | 已等價 | D-5 補入 |
| G5 | ReadPairRecord（gateway.pairingFileData 原始 bytes） | 已等價 | usbmuxd_proxy.rs |
| G6 | 未知 MessageType 處置 | 已等價 | D-3：Swift 碼為吞錯續讀（INVENTORY 原註記有誤），Rust 從碼 |
| G7 | notifyDeviceAttached/Detached | 已等價 | FFI `mm_proxy_notify_attached/detached` |
| G8 | RawPacket 16B header＋size 界（16..4095） | 已等價 | usbmuxd_proxy 封包層；fixtures 測 |
| H1 | EMProxyAPI.start(host:port:) | 顯名偏離 | mm-emproxy `EmProxy::start`（em_proxy crate 相依、釘 rev 6e117e14＝v0.9.3 同源）；D-M6-8（C 層全域單例不複刻，App 可達情境等價） |
| H2 | EMProxyAPI.start() 預設版（127.0.0.1:51820） | 已等價 | `mm_emproxy_start_default` |
| H3 | setHandshakeClient | 已等價 | `EmProxy::set_handshake_client`；未設而 start 回 handshakeClientNotConfigured |
| H4 | stop | 已等價 | `EmProxy::stop`；錯誤碼 -1..-3 映射表測 |
| H5 | triggerVPNHandshake（指定 peer 每 200ms×10s） | 已等價 | mm-emproxy handshake probe；handshake_probe 6 測 |
| H6 | probeVPNHandshake（掃 utun 候選並行探 62078、.waiting＋ECONNREFUSED 視為通） | 顯名偏離 | D-M6-3（首勝時延差）／D-M6-4（waiting 中間態在 BSD 複刻塌縮，由 200ms 重試吸收） |

## §5 無線配對與 DDI（INVENTORY 2.9／2.10）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| I1 | WirelessPairAPI.onPinReceived | 已等價 | 事件橋→sink `on_pin_received`；wireless_pair 測 |
| I2 | onReadyToPair(serviceID, port) | 已等價 | D-M8-12（TXT 不達 App，與 Swift 一致；三元組在 gateway→service 界面原樣） |
| I3 | onRequestPin（requestId 對應、未掛回呼預設 "000000"） | 已等價 | D-M8-4（60s 逾時代用在 gateway 層，Swift 同位） |
| I4 | wirelessPair.start | 顯名偏離 | D-M8-2（真取消 vs Swift 阻塞 FFI 取消無效）；廣告 D-M8-9 |
| I5 | wirelessPair.trigger（keyed 任務、同 key 重入取消舊） | 顯名偏離 | D-M8-2／D-M8-11（清槽 id 比對硬化） |
| I6 | wirelessPair.stop | 顯名偏離 | D-M8-2（stop 真取消＋停廣告） |
| I7 | PairedDeviceRecord 產出與配對檔落盤 | 已等價 | D-M8-10（finalize 死參數不設、record 四欄一致；pairingFile 過界以序列化內容交付） |
| I8 | Bonjour 廣告 `_remotepairing-pairable-host._tcp.`＋TXT | 已等價 | mm-rp advertise.rs 直呼 dns-sd C API（主路線）；D-M8-9：macOS CI spike 真註冊通過、**E2 後備殼不啟用** |
| J1 | Mounter.mount 調度（路徑正規化、endpoint 前置、已掛回 false／新掛回 true） | 已等價 | mm-core mounter.rs |
| J2 | pre-17 路（本地檔／versions.json 下載 zip） | 顯名偏離 | D-M5-6 僅 gw-limd 掛載步（暫存＋upload＋mount）；下載與查表等價 |
| J3 | personalized 路（三件下載／RSD 與直連分流） | 顯名偏離 | D-M5-7（gw-limd 經 idevice 引擎 over 代理）；gw-idevice 兩路等價 |
| J4 | mountTask 管理（stop/重掛可取消等待） | 已等價 | CoreState.mount_task（D-M4-4 已由 M5 結案） |
| J5 | debug 前置 ensureDDIMounted | 已等價 | core debug_app/attach_debugger 前置 |

## §6 DeviceGateway 協議與兩後端（INVENTORY 2.11）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| K1 | requiresUsbmuxd（idevice false／limd true） | 已等價 | `GatewayBackend::requires_usbmuxd`；base_core 測 |
| K2 | pairingFileType | 已等價 | GatewayCore（BaseDeviceGateway 等價狀態） |
| K3 | pairingFileData | 已等價 | GatewayCore（didSet 衍生 dict 語意同） |
| K4 | pairingDataDict | 已等價 | GatewayCore（解析失敗→None） |
| K5 | servicePort（extension） | 已等價 | trait `service_port()`＝get_port(現型別) |
| K6 | getPort（缺值回 defaultPort） | 已等價 | GatewayCore 埠表；.unknown 依 ios17 建構參數（目標 iOS 15 預設 false，M3 既定） |
| K7 | setPort（同值 no-op、異值 invalidate） | 已等價 | GatewayCore generation 惰性失效（D 表 M3 §1） |
| K8 | gateway.start（雙模式解析） | 顯名偏離 | D-M7-1（gw-limd RP 驗證條件；照抄 Swift 即死碼）；PairingError 原樣上抛同 Swift |
| K9 | gateway.stop（cleanup、不清埠表） | 已等價 | GatewayCore cleanup 逐字 |
| K10 | setDeviceEndpointIp（異值 invalidate） | 已等價 | GatewayCore |
| K11 | gateway.setLogging | 顯名偏離 | D-9（同 A19） |
| K12 | fetchUDID | 已等價 | 兩後端 lockdown＋RP 四路；mock_lockdown 測 |
| K13 | getLockdownValue | 已等價 | 兩後端；session 先行與錯誤分類逐段複刻（m3 diff §2/§3） |
| K14 | isDDIMounted（gateway） | 已等價 | 兩後端覆寫（M5） |
| K15 | mountDeveloperImage | 顯名偏離 | D-M5-6（gw-limd） |
| K16 | mountPersonalizedDdi | 顯名偏離 | D-M5-7（gw-limd） |
| K17 | installProvisioningProfile | 顯名偏離 | D-M5-14（gw-limd 描述子消費：misagent 精確複刻、其餘以 wrapper 型建構子替代） |
| K18 | removeProvisioningProfile | 已等價 | 兩後端 misagent 路 |
| K19 | dumpProfiles（gateway） | 已等價 | 回傳形狀逐後端複刻（idevice 目錄路徑／limd XML 字串） |
| K20 | removeApp | 顯名偏離 | D-M5-2（gw-idevice 自寫 instproxy） |
| K21 | sendIpaAfc | 顯名偏離 | D-M5-15（單次 AFC Write 全量送出；裝置上限待真機） |
| K22 | sendAppBundleAfc | 顯名偏離 | D-M5-13／D-M5-3（自寫 AfcSession） |
| K23 | installIpa | 顯名偏離 | D-M5-2 |
| K24 | installAppBundle | 顯名偏離 | D-M5-2 |
| K25 | wipeContainer | 顯名偏離 | D-M5-8；Swift 原行為「VendContainer 僅止於此、不刪檔」已複刻 |
| K26 | debugApp | 顯名偏離 | D-M5-5／D-M7-9（RspClient）；launchAppPre17 分流在內 |
| K27 | debugProcess | 已等價 | lockdown idevice 同形 unsupported（D-M5-1）；RP 路真實作（M7）；gw-limd 兩路真實作 |
| K28 | performHeartbeat（gateway） | 已等價 | 同 C1 |
| K29 | startWirelessPair（gateway） | 已等價 | gw-idevice 真實作；gw-limd 同形 unsupported（G12，附錄 A-a2） |
| K30 | triggerWirelessPair（gateway） | 已等價 | 同 K29 |
| K31 | afcListDirectory | 顯名偏離 | D-M7-4（gw-limd 無 RP 路）；gw-limd 忽略 bundleId 照 Swift（m5 diff） |
| K32 | afcReadFile | 顯名偏離 | D-M7-4 |
| K33 | afcGetFileInfo | 顯名偏離 | D-M7-4 |
| K34 | Base RSP 解析（RSPStopReply/RSPProcessInfo） | 已等價 | mm-gateway rsp.rs 逐函數＋單元測 6 |
| K35 | Base findProcessPID（vAttachName 逐詞） | 已等價 | rsp.rs／gw 呼叫點 |
| K36 | Base stringToHex/decodeHexASCII | 已等價 | rsp.rs（小寫 hex 逐字） |
| K37 | limd DeviceService 8 枚舉服務名 | 顯名偏離 | D-M7-10（misagent RSD 名 MCInstall 形 vs crate misagent 形，真機目錄實名待驗） |
| K38 | 服務連接策略（idevice 四路／limd 兩套） | 顯名偏離 | D-1（performWithUsbmuxdService 死路徑以 helper 實作、生產路由照 Either）；D-M7-7／D-M7-8（RP 重試落點） |

## §7 Common、常數與型別（INVENTORY 2.12／3.3）

| # | Swift 成員 | 態 | Rust 落點／證據 |
|---|---|---|---|
| L1 | PairingProtocol 3 例＋defaultPort | 已等價 | mm-common pairing.rs（rppairing 49152／lockdown 62078／unknown 依 iOS 17 分流） |
| L2 | PairingFile 協議（rawContent/rawData/mode/plist） | 已等價 | mm-common `PairingFile` |
| L3 | RPPairingFile 必填鍵（private_key/public_key/identifier） | 已等價 | pairing.rs；pairing 7 測含 fixtures 八形狀 |
| L4 | LockdownPairingFile 必填鍵 10 個＋抽出欄位 | 已等價 | pairing.rs |
| L5 | PairingFileParser.validate（雙齊 ambiguous／雙缺 invalidPlist 列缺鍵） | 已等價 | pairing.rs |
| L6 | PairingFileParser.parse | 已等價 | pairing.rs；FFI `mm_parse_pairing_file` |
| L7 | MinimuxerConstants 約 33 常數 | 已等價 | mm-common constants.rs 逐值（constants 4 測逐值斷言）；含心跳三常數、Bonjour 三型別、DDI 三 URL |
| L8 | NetworkUtils.testTCP（非阻塞 connect+poll+SO_ERROR、拒 "/"、v6 scope） | 已等價 | mm-common probe.rs `test_tcp`／`probe_tcp_outcome`（M6 errno 複刻）；probe 5 測 |
| L9 | TCPAcceptor（bind/listen、埠 0 隨機、getsockname 回讀） | 已等價 | mm-rp pairing.rs `TcpAcceptor`（M8 無線配對 acceptor 本體；DESIGN 風險 12「無使用點不移植」已被 M8 使用點推翻、按使用點實作）；pairing 測含 bind/accept |
| L10 | withFFIDispatch | 顯名豁免 | Swift 側 GCD 卸載形狀；Rust 側 tokio runtime 原生承載、swift-bridge async 導出即其等價面（殼層段沿用） |
| L11 | matchingPriority | 顯名豁免 | 同 L10（detached task 優先級形狀；Rust 任務在自有 runtime） |
| L12 | ConcurrencyUtils.toSendableDictionary | 顯名豁免 | 套件內部輔助；Rust 以 plist Dictionary 型別直接承載（GatewayCore pairing_data_dict） |
| L13 | Logging（模組 Logging enum＋debugLog/verboseLog 自由函數） | 已等價 | mm-common logging.rs（debug/verbose 兩級＋sink 路由）；D-M6-5（EMProxy 層級對映逐字） |
| L14 | MinimuxerComponent（heartbeat/mounter） | 已等價 | error-map MinimuxerServiceError 兩例的 component 載體 |
| L15 | ProfileDumpMode（zip/raw） | 已等價 | core dump_profiles mode 參數 |
| L16 | DeviceConnectionMode 3 例 | 已等價 | mm-net types.rs |
| L17 | ServicePort（protocolType+port） | 已等價 | mm-net types.rs；resolveServicePort 過界形狀同 |
| L18 | ConnectionConfigBinding 11 閉包 | 已等價 | FFI `MmConnectionConfig` 11 方法（7 setter＋3 getter＋resolve_service_port 請求/回應）；manager 消費全套 |
| L19 | GatewayBackend 2 例（單例預設 idevice） | 已等價 | hub 預設 idevice；UserDefaults 讀取屬 App 側 |
| L20 | iOS 26.4 ipsec 關卡（#available 等價） | 已等價 | sysctl kern.osproductversion 解析（core os_version 單元測）；iOS 15 恆不觸發，DESIGN 風險 10 保留裁示 |

## §8 錯誤案例 73 例逐例核對（error-map.toml 為單一真值）

核對法：toml 每例的 domain/code/description 與 Swift 原定義逐字比對
（MinimuxerError 以 swift-src/Sources__MinimuxerError.swift 為據；其餘
domain 以 INVENTORY 2.11/2.12 與各段 diff 的逐字引文為據），並經
mm-common error_map.rs 5 測（全表往返＋description 渲染）鎖定。

| # | domain | code | description 模板 | 核對 |
|---|---|---|---|---|
| 1 | MinimuxerError | noDevice | NoDevice: {reason} | 一致 |
| 2 | MinimuxerError | noConnection | NoConnection: {reason} | 一致 |
| 3 | MinimuxerError | notReachable | NotReachable: {reason} | 一致 |
| 4 | MinimuxerError | noVPN | NoVPN: {reason} | 一致 |
| 5 | MinimuxerError | notStarted | NotStarted: {reason} | 一致 |
| 6 | MinimuxerError | pairingNotLoaded | PairingNotLoaded: {reason} | 一致 |
| 7 | MinimuxerError | connectionModeNotConfigured | ConnectionModeNotConfigured: {reason} | 一致 |
| 8 | MinimuxerError | restartAlreadyInProgressError | RestartAlreadyInProgressError: {reason} | 一致 |
| 9 | MinimuxerError | invalidVPN | InvalidVPN: {reason} | 一致 |
| 10 | MinimuxerError | invalidPairing | InvalidPairing(protocol: {protocol}, reason: {reason}) | 一致 |
| 11 | MinimuxerError | muxerNotListening | MuxerNotListening: {reason} | 一致 |
| 12 | MinimuxerError | createDebug | CreateDebug: {reason} | 一致 |
| 13 | MinimuxerError | createInstproxy | CreateInstproxy: {reason} | 一致 |
| 14 | MinimuxerError | createLockdown | CreateLockdown: {reason} | 一致 |
| 15 | MinimuxerError | createCoreDevice | CreateCoreDevice: {reason} | 一致 |
| 16 | MinimuxerError | createSoftwareTunnel | CreateSoftwareTunnel: {reason} | 一致 |
| 17 | MinimuxerError | createRemoteServer | CreateRemoteServer: {reason} | 一致 |
| 18 | MinimuxerError | createProcessControl | CreateProcessControl: {reason} | 一致 |
| 19 | MinimuxerError | getLockdownValue | GetLockdownValue: {reason} | 一致 |
| 20 | MinimuxerError | fetchUDID | FetchUDID: {reason} | 一致 |
| 21 | MinimuxerError | connect | Connect: {reason} | 一致 |
| 22 | MinimuxerError | close | Close: {reason} | 一致 |
| 23 | MinimuxerError | xpcHandshake | XpcHandshake: {reason} | 一致 |
| 24 | MinimuxerError | noService | NoService: {reason} | 一致 |
| 25 | MinimuxerError | invalidProductVersion | InvalidProductVersion: {reason} | 一致 |
| 26 | MinimuxerError | lookupApps | LookupApps: {reason} | 一致 |
| 27 | MinimuxerError | findApp | FindApp: {reason} | 一致 |
| 28 | MinimuxerError | bundlePath | BundlePath: {reason} | 一致 |
| 29 | MinimuxerError | maxPacket | MaxPacket: {reason} | 一致 |
| 30 | MinimuxerError | workingDirectory | WorkingDirectory: {reason} | 一致 |
| 31 | MinimuxerError | argv | Argv: {reason} | 一致 |
| 32 | MinimuxerError | launchSuccess | LaunchSuccess: {reason} | 一致 |
| 33 | MinimuxerError | detach | Detach: {reason} | 一致 |
| 34 | MinimuxerError | attach | Attach: {reason} | 一致 |
| 35 | MinimuxerError | createAfc | CreateAfc: {reason} | 一致 |
| 36 | MinimuxerError | rwAfc | RwAfc: {reason} | 一致 |
| 37 | MinimuxerError | installApp | InstallApp({reason}) | 一致 |
| 38 | MinimuxerError | uninstallApp | UninstallApp: {reason} | 一致 |
| 39 | MinimuxerError | createMisagent | CreateMisagent: {reason} | 一致 |
| 40 | MinimuxerError | profileInstall | ProfileInstall: {reason} | 一致 |
| 41 | MinimuxerError | profileRemove | ProfileRemove: {reason} | 一致 |
| 42 | MinimuxerError | createFolder | CreateFolder: {reason} | 一致 |
| 43 | MinimuxerError | downloadImage | DownloadImage: {reason} | 一致 |
| 44 | MinimuxerError | imageLookup | ImageLookup: {reason} | 一致 |
| 45 | MinimuxerError | imageRead | ImageRead: {reason} | 一致 |
| 46 | MinimuxerError | mount | Mount(protocol: {protocol}, reason: {reason}) | 一致 |
| 47 | DeviceGatewayError | invalidPairingFile | The pairing file is invalid: {reason} | 一致 |
| 48 | DeviceGatewayError | connectionFailed | Failed to connect to device: {reason} | 一致 |
| 49 | DeviceGatewayError | serviceError | Service operation failed: {reason} | 一致 |
| 50 | DeviceGatewayError | noConnection | {reason} | 一致 |
| 51 | DeviceGatewayError | notInitialized | {reason} | 一致 |
| 52 | DeviceGatewayError | deviceEndpointIpNotAvailable | {reason} | 一致 |
| 53 | DeviceGatewayError | invalidTargetEndpoint | {reason} | 一致 |
| 54 | DeviceGatewayError | unsupportedOperation | Operation '{reason}' is not supported. | 一致 |
| 55 | PairingError | unreadable | The pairing file could not be read: {reason} | 一致 |
| 56 | PairingError | invalidPlist | The pairing file could not be parsed as a property list (plist): {reason} | 一致 |
| 57 | PairingError | incomplete | The pairing file is incomplete for .{protocol}. Missing keys: {missing_keys}. | 一致 |
| 58 | PairingError | ambiguous | The pairing file format is ambiguous: {reason} | 一致 |
| 59 | EMProxyError | invalidBindAddressPointer | Invalid bind address pointer | 一致 |
| 60 | EMProxyError | invalidUTF8String | Failed to convert bind address to UTF-8 | 一致 |
| 61 | EMProxyError | invalidSocketAddress | Invalid IPv4 socket address format: {reason} | 一致 |
| 62 | EMProxyError | socketBindFailed | Failed to bind to UDP socket address | 一致 |
| 63 | EMProxyError | cryptoInitFailed | Failed to initialize EMProxy crypto keys | 一致 |
| 64 | EMProxyError | serverNotRunning | EMProxy server is not running | 一致 |
| 65 | EMProxyError | stopSignalFailed | Failed to send stop signal to EMProxy server | 一致 |
| 66 | EMProxyError | threadJoinFailed | Failed to join EMProxy loopback thread | 一致 |
| 67 | EMProxyError | handshakeClientNotConfigured | EMProxy WireGuard VPN handshake client not configured | 一致 |
| 68 | EMProxyError | unknownError | EMProxy error code: {code} | 一致 |
| 69 | MinimuxerInternalError | deviceEndpointNotInitialized | DeviceEndpointNotInitialized | 一致 |
| 70 | MinimuxerInternalError | networkIfaceNotRefreshed | NetworkIfaceNotRefreshed | 一致 |
| 71 | MinimuxerInternalError | pairingFailed | PairingFailed({reason}) | 一致 |
| 72 | MinimuxerServiceError | heartbeat | [{component}] {reason} | 一致 |
| 73 | MinimuxerServiceError | mounter | [{component}] {reason} | 一致 |

補充核對：
- MinimuxerError 46 例：Swift 源 case 數 46（INVENTORY 原記 44 為
  M1 更正，總數 73 不變：46+8+4+10+3+2）。description 模板逐例
  與 Swift `description` switch 一致，含 installApp 的無冒號形
  `InstallApp({reason})` 與 invalidPairing/mount 的 protocol 載荷形。
- DeviceGatewayError 8 例：含 noConnection 等四例的「空 reason 用
  預設文案」語意（toml description_empty_reason 欄），與 Swift 一致。
- 映射邏輯：isVPNDrop（broken pipe/connection reset/early eof/
  unexpected eof/no route to host/connection refused＋code 限
  connectionFailed/serviceError）、isRetryable、asMinimuxerError
  於 mm-common error.rs 實作，core_lifecycle map_gateway_error_table
  測全表鎖定；BrokenPipe/early eof→invalidVPN 為今晨對話框成形路徑。
- 結論：73/73 一致，無缺例、無多例、無文案漂移。

## §9 ready() DDI 語意落實確認（DESIGN 風險 9）

覆核結論：**與 Swift 現行語意逐字一致，無出入、不需修正。**

- 開關名：Swift `isReady(withNetworkCheck:withDDIMountCheck:)`，
  extension 預設 `withNetworkCheck=true, withDDIMountCheck=false`；
  Rust `MinimuxerCore::is_ready(with_network_check, with_ddi_mount_check)`，
  FFI `mm_core_is_ready` 同參數序，狀態重探路徑以 (true, false) 呼叫。
- 落點：九關全過後才執行第十關——`if with_ddi_mount_check { return
  is_ddi_mounted().await }`，即以 isDDIMounted 結果為成敗，與 Swift
  「最後若 withDDIMountCheck，再以 isDDIMounted() 結果為成敗」同位同形。
- 預設不計：App 與 publisher 重探皆走預設參數，DDI 未掛不影響就緒，
  與 Swift 一致（INVENTORY 2.2：withDDIMountCheck=true 路徑 App 未用）。
- 註記：舊 Rust `ready()` 的註解/實作不一致（風險 9 原案）已不對外；
  現行就緒面只有上述 Swift 語意一條路。
- 已知形狀差（非 DDI 語意本身）：第十關回 `.success(false)` 時，
  狀態推播面無法攜帶 Bool 值——D-M4-3 已裁示，列於 A14。

## §10 漂移根因儀器完備性覆核（DESIGN 風險 11，只備儀器不定讞）

覆核結論：**主儀器齊備；補一處日誌後全覆蓋。** 定讞屬真機場
（同場景抓路由狀態＋勝出候選），本段不下結論、不改行為。

齊備的部分：

- derived 候選勝出：`resolve_local_vpn_tunnel` 勝出即印
  `candidate peer <ip> won via <gateway|destination|link-layer>
  (mask: <mask>)`——來源與 mask 同行，真機漂移場景（10.7.0.0/24）
  可直接判讀是哪條優先序產出網段位址。
- 每一次探測：`probe_with_source` 印 `tcpProbe <ip>:<現埠>
  (protocol: .<型別>) -> reachable/unreachable`，RP 模式埠（49152）
  與 lockdown 埠（62078）自動入誌（埠自 port_source 現值）。
- 路由/介面狀態：refresh 時 verbose 印全介面清單（含每 tunnel 的
  destinationRoutes），debug 印 refresh 彙總（iface IP/mask、
  reported gateway、derived peer/mask、override 與其可達性）；
  remoteServer 模式另有專屬彙總行。
- override 與 remote 探測同經 `probe_with_source`，有 tcpProbe 行
  可對照；endpoint 採用/清除由 observer refresh_endpoint 彙總行呈現。

本段補的缺口（僅日誌、零行為變更）：

- `probe_with_source` 的 resolveServicePort 換埠路徑原先靜默——
  換埠勝出時只見最終 tcpProbe 行，無法分辨是原埠直通還是換埠通。
  已補：`candidate peer <ip> won via resolveServicePort
  (port <舊> -> <新>)`。此路同時是 RP 模式探 49152 失敗後向 App
  解析新埠的路徑，覆蓋任務書點名的 RP 與換埠兩格。

## 附錄 A：unsupported 殘留全量處置（2026-11-18 grep 實查）

grep 範圍：全 workspace `crates/*/src` 的 `unsupportedOperation`／
`unsupported(` 呼叫點（tests 內的替身樁不計入產品面，僅列 t3 說明）。

### (a) Swift 本身即 unsupported 的同形複刻——保留，計 3 點

- a1 `gw_idevice.rs debug_process`（lockdown 分支）：Swift idevice
後端同點抛 unsupportedOperation，reason 逐字
"Attaching to process by PID is only supported in.rppairing mode"
（原文含 ".rppairing" 前少空格的 typo，逐字複刻）；RP 分支為真
實作。D-M5-1。對照表 A36/K27 標。
- a2 `gw_limd.rs start_wireless_pair`、a3 `gw_limd.rs
trigger_wireless_pair`：Swift limd 後端兩入口皆抛
unsupportedOperation，reason 逐字 "… (RemotePairing is not
supported on pure Lockdown gateway)"（行為清單 G12）。
對照表 K29/K30 標。

### (b) 過渡殘留——本段清理，計 3 處（皆註解/文件，無行為面）

- b1 `mm-gateway/src/gateway.rs` trait 預設方法群（20 個
`unsupported(...)` 預設體）：M3–M8 期間的過渡形狀，現兩後端
已全數覆寫（gw-idevice 於 gw_idevice.rs、gw-limd 於 gw_limd.rs
逐一核對，含 RP 與無線段）。預設體保留給 test doubles（核心與
gateway 測試的替身 gateway），屬 trait 設計而非產品缺口；
本段清理其過期註解（"until M7"、"RP paths stay unsupported"
等三處）為現況文案。
- b2 `mm-gateway/src/lib.rs` 模組文件「RP 路徑 M7 前回
unsupported」段：已改寫為現況（指向本附錄）。
- b3 `mm-core` 兩處防禦性回退：`core.rs` 心跳 provider 無 gateway
時回 `unsupported("performHeartbeat")`（生產恆有 gateway，
僅未 start 的防禦格）；`wireless_pair.rs` 任務 oneshot 通道
斷裂時回 `unsupported("startWirelessPair")`／
`("triggerWirelessPair")`（任務 panic 才可達的兜底）。
兩處皆非 Swift 對應面，保留並在此顯名。

### (c) 真缺口——計 0 點

全 grep 呼叫點逐點核對後，沒有任何「後段已實作卻仍回
unsupported」或「從未實作」的產品面呼叫點。

### tests 樁（不計入產品面）

`mm-core/tests`、`mm-gateway/tests` 內替身 gateway 以
`unsupported(...)` 模擬未覆寫操作，屬測試構造，保留。

## 結算

- 對照表（§1–§7）共 **157 行**：110、43、
4、**0**。
- 錯誤案例（§8）：**73/73** 與 Swift 原定義一致。
- unsupported 殘留：(a) 同形 3 點、(b) 清理 3 處、(c) 缺口 0 點。
- ：**無**。本段覆核未發現行為級錯誤；§10 的
日誌補強為儀器補完（DESIGN M9 明許的儀器工作），非行為修正。
- DDI 語意（§9）：逐字一致。漂移儀器（§10）：補一處後全覆蓋。
