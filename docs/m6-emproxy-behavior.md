# M6 行為清單：EMProxy（子系統 8）

日期：2026-11-18。依 DESIGN §4 M6 與風險 5：本清單是 EMProxy 的規格
來源（行為以測試與封裝源碼為準，不以 em_proxy 內部實作想像為準）。
M6 完成條件＝本清單逐項有對應驗證（cargo test 編號或真機驗收項）。

規格來源（逐行讀畢）：
- `yongshuai1013/minimuxer` @714aafa3：`Tests/EMProxyTests.swift`
  （20,409 B，blob 3aba78f900）、`Sources/EMProxyImpl.swift`
  （8,898 B，blob 920e57694b）、`Sources/MinimuxerApi.swift` 的
  `EMProxyAPI` 協議與 extension（blob 同 M4 已讀）。
- `yongshuai1013/SideStore` @develop 8ec3def1：
  `SideStore/Core/DeviceApi/EMProxyWrapper.swift`、
  `SideStore/AppBootManager.swift`（App 側調度語義）。
- `SideStore/em_proxy` @v0.9.3（＝develop 6e117e14）：`src/lib.rs`
  （伺服器行為的交叉參照；Package.swift binaryTarget 釘的
  xcframework 即此版，checksum 3998789c…）。

架構註記（Tests 檔頭封包流圖）：App（10.7.0.2）→ VPN client →
EMProxy（boringtun WG server，UDP 127.0.0.1:51820）→ 解封後封包
經「loopback 交換」回到本機 stack → lockdownd :62078。EMProxy 本體
是 WG server：接 VPN client 的隧道封包，解封後把 IPv4 src/dst
對調再封裝送回，使封包以「來自目的地」的樣貌由 VPN client 交付
本機協定棧。

## A. 封裝面（EMProxyImpl / EMProxyAPI）

- **A1 設定存放**：`setHandshakeClient(host, port, enabled)` 把三元組
  存入實例（NSLock 保護、可覆寫）；兩參便捷版
  `setHandshakeClient(host, port)`＝`enabled: true`。
  驗證：cargo `config_and_state::a1_config_store_and_replace`。
- **A2 預設 start**：無參 `start()`＝`start(host: empServerHost,
  port: empServerPort)`＝127.0.0.1:51820（mm-common 常數
  EMP_SERVER_HOST／EMP_SERVER_PORT，M1 已移植）。
  驗證：cargo `config_and_state::a2_default_start_binds_51820`
  （以 UDP 佔位檢查 51820 被綁；與 w 組測試共用埠鎖，見 §F）。
- **A3 未設定先 start**：`start` 時 handshakeConfig 為 nil →
  抛 `EMProxyError.handshakeClientNotConfigured`，且伺服器不得啟動
  （埠未被綁）。驗證：cargo
  `config_and_state::a3_start_without_config_throws`。
- **A4 start 錯誤映射**：start 把 `"\(host):\(port)"` 字串交 C 層，
  回碼映射：0→成功；-1→invalidBindAddressPointer；
  -2→invalidUTF8String；-3→invalidSocketAddress（payload＝完整
  `"host:port"` 字串）；-4→socketBindFailed；-5→cryptoInitFailed；
  其餘→unknownError(code)。
  驗證：cargo `config_and_state::a4_start_code_mapping_table`
  （映射函數全表，含未知碼透傳）；-3 以 IPv6 host（`"::1"` 組出的
  `"::1:51820"` 非法 SocketAddrV4）觸發：
  `a4_start_ipv6_host_is_invalid_socket_address`；-4 以預先佔用
  UDP 埠觸發：`a4_start_occupied_port_is_bind_failed`。
  （-1/-2 是 C 字串層錯誤，Rust 面以 `&str` 進入、不可達，僅映射表
  保留；-5 需金鑰解析失敗，出貨金鑰下不可達，同上。）
- **A5 start 冪等**：伺服器已在跑時再 start → C 層回 0（記錄
  "Proxy already exists, skipping"）、不重綁、不換實例。
  驗證：cargo `config_and_state::a5_start_twice_is_idempotent`
  （兩次 start 皆 Ok，其後 WG 握手仍可通，見 w1）。
- **A6 stop 錯誤映射**：stop 回碼：0→成功；-1→serverNotRunning；
  -2→stopSignalFailed；-3→threadJoinFailed；其餘→unknownError。
  未在跑時 stop → serverNotRunning。
  驗證：cargo `config_and_state::a6_stop_code_mapping_table`、
  `a6_stop_when_not_running`。
- **A7 stop 後狀態**：stop 成功後 UDP 埠釋放（可被外部綁定）、
  可再次 start 並正常服務。
  驗證：cargo `config_and_state::a7_stop_releases_port_and_restart`。
- **A8 log 回呼**：EMProxyImpl init 註冊 C log 回呼：訊息加前綴
  `"[EMProxy] "` 後，level ≤ 1 → verboseLog、level ≥ 2 → debugLog
  （em_proxy 的 level 語義：0=DEBUG、1=INFO、2=WARN、3=ERROR，
  見其 em_proxy.h）；msg 指標為 nil 時回呼回 false（不消費）。
  回呼回 true 表示已消費（em_proxy 內部不再 fallback 打印）。
  Rust 面：mm-emproxy 於首次使用時註冊一次（Once），轉發進
  mm-common logging 門面、規則同上、恒回 true；nil 訊息不可達
  （em_proxy 只以 CString 呼叫）。
  驗證：cargo `config_and_state::a8_log_forwarding_levels`。

## B. 握手探測（start 成功後；結果不影響 start 的成功與否）

start 內 C 層回 0 後才探測；探測逾時／失敗僅記錄，**不抛錯**。

- **B1 分流真值表**（EMProxyImpl.start 尾段）：
  `config.enabled && !config.host（去空白後）非空` →
  triggerVPNHandshake(host, config.port)；其餘組合（enabled=false、
  或 host 空）→ probeVPNHandshake(埠＝config.port≠0 ? config.port
  : lockdowndPort 62078)。
  驗證：cargo `handshake_probe::b1_dispatch_table`（四組合以注入的
  候選源與短化時參斷言走哪一路、探哪一埠）。
- **B2 trigger 路**：對指定 peer 反覆 probeHost，每輪間隔 200ms，
  直到成功或總時長達 vpnHandshakeTimeoutNs＝10s；成功即返回。
  驗證：cargo `handshake_probe::b2_trigger_succeeds_on_listener`、
  `b2_trigger_refused_counts_as_success`、
  `b2_trigger_times_out_without_throwing`（短化總時長；start 仍 Ok）。
- **B3 probe 路（候選掃描）**：候選＝NetworkIfaceScanner.scan →
  DeviceConnectionManager.resolveCandidatePeers → ip 列表；候選為空
  → 立即跳過（僅記錄）。每輪以 TaskGroup 並行探全部候選、首勝即
  該輪成功並 cancelAll；輪間隔 200ms，總時長 10s；逾時僅記錄。
  驗證：cargo `handshake_probe::b3_probe_path_uses_injected_candidates`
  （注入 [127.0.0.1]＋listener）、
  `b3_probe_path_no_candidates_skips_fast`（注入空表，耗時≪總時長）。
  生產路徑的候選源由 mm-core 接線（mm-net scan＋resolve，見
  m6-emproxy-diff.md 的接線裁示）；真機候選正確性併入 D 組。
- **B4 probeHost 單次語義**：NWConnection TCP 連線，1 秒上限：
  .ready→成功；.waiting 且 POSIX 錯誤 61（ECONNREFUSED）→**成功**
  （隧道已通、只是沒人在聽）；.waiting 其他錯誤→續等至上限；
  .failed/.cancelled→失敗；逾時→失敗。
  Rust 複刻（DESIGN M6 指定）：BSD 非阻塞 connect＋poll 讀
  SO_ERROR：0→Connected（成功）；ECONNREFUSED（Darwin 61，
  Rust 以 libc::ECONNREFUSED 平台值）→Refused（成功）；其餘／
  逾時→Failed。原語在 mm-common probe 擴充 `probe_tcp_outcome`，
  既有 `test_tcp` 行為不變。
  驗證：cargo（mm-common）`probe::outcome_connected_refused_failed`
  與（mm-emproxy）`handshake_probe::b4_probe_host_outcomes`。

## C. 伺服器本體（em_proxy；規格＝Tests Test-2 ＋ lib.rs 交叉參照）

- **C1 WG 握手應答**（Tests `testEMProxyWireGuardServerHandshake_v040`
  本體）：handshake client 設為 disabled 後 start（預設
  127.0.0.1:51820），以 UDP 送 WireGuard handshake initiation
  （type 1、sender index 1、ephemeral＝全零私鑰之公鑰、加密 static
  ＝測試 client 私鑰 `AIIeeUDvk3NeAFJ9BWCQvPJize/9WZibMnGJ/0rt5k4=`
  的公鑰、加密 TAI64N timestamp、mac1 以 server 公鑰
  `kHDoekeYhBvfW9a9UQ+UCmpbG423eejTjcjW+DT+JF0=` 推導、mac2 全零；
  全長 148 B）→ 2 秒內收到 handshake response（≥60 B；server
  index＝[4..8]、server ephemeral 公鑰＝[12..44]、encrypted
  nothing＝[44..60]），且以 Noise 密鑰排程（BLAKE2s hash/HMAC 鏈
  ＋X25519 雙 DH＋ChaChaPoly）驗證 encrypted nothing 可解開。
  驗證：cargo `wireguard_server::c1_handshake_response_crypto_verifies`
  （Swift Test-2 客戶端邏輯的 Rust 直譯；於測試專用埠執行）。
- **C2 keepalive 受理**：握手後送 transport keepalive（type 4、
  receiver＝server index、counter 0、以 session sending key
  ChaChaPoly 封裝空 payload，全長 32 B）→ 伺服器無錯誤反應、
  保持存活（其後 stop 回 Ok）。
  驗證：cargo `wireguard_server::c2_keepalive_accepted_server_alive`。
- **C3 金鑰對釘死**：伺服器靜態公鑰＝emp.conf [Peer] PublicKey
  （kHDo…JF0=），認可的 client 靜態公鑰＝emp.conf [Interface]
  私鑰（AIIe…5k4=）的公鑰（em_proxy 以 repo 內 keys/ 編譯期嵌入：
  server_privatekey＋client_publickey）。以**其他** client 金鑰組
  的 initiation → 無應答（decapsulate 拒絕）。
  驗證：cargo `wireguard_server::c3_unknown_client_key_gets_no_response`；
  C1 的應答可解開本身即證明 server 金鑰正確。
- **C4 loopback 交換**：已建連 session 送入 transport 封包（內層
  IPv4，src＝client 位址 10.7.0.10、dst＝10.7.0.1）→ 伺服器把
  內層 src/dst 對調後封裝送回同一 endpoint；client 解開後內層
  src/dst 與送出時相反。
  驗證：cargo `wireguard_server::c4_loopback_swaps_ipv4_src_dst`。
  （boringtun 對內層 src 的 allowed-IP 檢查若在 host 環境擋下，
  本項降級為真機 D1 覆蓋並在此註記實測結果。）
- **C5 雜訊韌性**：隨機垃圾 UDP 資料報 → 解封錯誤僅記錄、迴圈
  繼續；其後合法 initiation 仍獲應答。
  驗證：cargo `wireguard_server::c5_garbage_datagram_server_survives`。
- **C6 綁定同步性**：UDP bind 在 start 返回前同步完成（失敗即
  -4，不存在「start 成功但稍後才綁失敗」）；stop 以 channel
  訊號＋join 完成，伺服執行緒以 5ms socket 逾時輪詢訊號，
  stop 應在短時間內返回。
  驗證：A4 的 -4 測試＋A7 的 stop/rebind 測試覆蓋；
  `config_and_state::c6_stop_returns_promptly`（stop 耗時 < 2s）。
- **C7 頭檔殘留**：include/EMProxy/em_proxy.h 宣告
  `test_emotional_damage(int)`，但 v0.9.3 lib.rs **無此符號**、
  Swift 側亦無呼叫點 → 不在移植面（記錄性條目，無驗證）。

## D. 真機驗收（host 不可驗）

- **D1 全鏈路**（Tests `testEMProxyWireGuardDeviceHandshake`）：
  真 WG VPN client 已連（emp.conf、endpoint 127.0.0.1:51820），
  本機 dummy TCP listener 於 39483；setHandshakeClient
  (10.7.0.1, 39483)→start()（預設綁定）→以 NWConnection 連
  10.7.0.1:39483 送 "Hello World"，封包經 VPN→EMProxy loopback
  →dummy listener 收到原文；stop() 乾淨結束。15 秒逾時判準。
- **D2 冷啟鏈**（DESIGN M6 驗收）：AppBootManager 冷啟路徑
  startEMProxy→minimuxer start→fetchUDID 驗連通過（殼層收編
  後於整合段執行；同 M4 真機清單的註冊流程項併場）。
- **D3 候選探測真機值**：B3 的生產候選源（utun 掃描衍生）在
  真機隧道開/關場景的候選集合與 Swift 版同場景一致（併 M2
  真機遺留項同場）。

## E. App 調度語義（封裝面之外的既定行為，接線依據）

- **E1 啟動順序**（AppBootManager.startMinimuxer）：
  `enableEMPforWireguard` 開 → **先** `startEMProxy()`，其錯誤
  **直抛、中止整個 startMinimuxer**（minimuxerStart 不執行）；
  關 → 完全不碰 EMProxy。其後 minimuxerStart → fetchUDID 驗連
  （invalidPairing 才中止，其餘錯誤吞）。
  Rust 面：mm-core 的 start/stop **不**自動啟停 EMProxy（Swift
  MinimuxerImpl 同樣只持有 emproxy 實例、不在其 start/stop 內
  呼叫——Impl 全文已核）；順序由殼層 AppBootManager 等價物以
  FFI 面（mm_emproxy_* 先於 mm_core_start）複刻。整合段收編。
  驗證：cargo（mm-core）`core_lifecycle` 既有 start/stop 測試
  不受影響＋`m6_emproxy_not_driven_by_core_start`（core start/stop
  後 emproxy 仍未運行；emproxy 實例經 core.emproxy() 取得同一顆）。
- **E2 Wrapper 參數組裝**（EMProxyWrapper.startEMProxy）：
  bind_addr 預設 AppConstants.Proxy.serverURL；先 bindConnectionConfig
  → handshake peer＝overrideTunnelPeerIp（去空白非空優先）否則
  tunnelPeerIp ?? ""；setHandshakeClient(peer, 62078,
  enabled＝!peer.isEmpty)；再 emproxy.start(wireguardServerHost,
  wireguardServerPort)。bind_addr 的 "host:port" 拆分驗證
  （1–2 段，否則 invalidSocketAddress(bind_addr) 先抛）屬 App
  層，不在 Rust 面；Rust 面只收 (host, port) 與
  (handshake host, port, enabled) 原值。
  驗證：FFI 面形狀對應（mm-ffi 測試 `ffi_emproxy_face_shapes`）。

## F. 測試執行註記

- 伺服器是**程序級單例**（em_proxy GLOBAL_HANDLE）；cargo 測試
  以 mm-emproxy 內一把共用鎖串行化所有「會 start 伺服器」的
  測試，並各自使用獨立測試埠（A2 的 51820 預設綁定測試除外，
  須與其他用 51820 的測試互斥——同鎖覆蓋）。
- 探測時參（200ms/1s/10s）在測試以注入的短化 ProbeParams
  驗節奏語義；生產值＝Swift 常數原值（總時長取
  mm-common VPN_HANDSHAKE_TIMEOUT_NS）。
