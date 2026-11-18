# M7 補盤（M0(c)）：RemotePairing 協議與兩 gateway RP 段落逐函數對照

日期：2026-11-18。範圍：DESIGN §4 M7／§5.2、風險 2／3／4。
本文件是 M7 實作的硬前置：補盤裁示與證據先行，實作對照與
差異清單另見 `docs/m7-rp-diff.md`。

## 0. 來源與證據等級

| 來源 | 版本 | 取得方式 |
|---|---|---|
| Swift minimuxer（對照基準） | `yongshuai1013/minimuxer` @714aafa3 | 本機 `swift-src/` 兩 gateway 檔經 GitHub API 核對 git blob SHA 與遠端逐字一致（IdeviceGateway `b3bf9bb4b42e`、LibimobiledeviceGateway `e3ec529333ee`） |
| RemotePairingKit | `mahee96/RemotePairingKit` @main | API 逐檔下載全源碼（26 檔，含 src/*.cpp/h、include/*.h）逐行讀 |
| idevice crate 0.1.29 | crates.io（現行釘版） | 本機 registry 源碼逐項 grep |
| idevice crate 0.1.68 | crates.io 最新版（2026-09-15 發布） | crates.io API（feature 表）＋docs.rs 模組表＋registry 源碼逐項讀 |

**關鍵事實先行**：RemotePairingKit 不是 Swift 套件，而是
**C++ 函式庫**（OpenSSL 密碼學、自帶用戶態 TCP 協議棧），
經 C API（`include/rppairing/*.h`）供 Swift limd gateway 連結。
Swift idevice gateway 不用它——它呼叫 IDevice FFI
（SideStore/idevice v0.1.68-ss）的 `*_connect_rsd` /
`tunnel_create_rppairing_*` / `rp_pairing_file_*` 函數群。
兩條 RP 路徑在線路上是同一協議（jkcoxson 的 rppairing 規格，
其個人網站有 spec 文；RPK 是 Magesh K 的獨立 C++ 實作）。

## 1. 裁示摘要（詳證據見 §6）

**裁示：(a) 接線＋補缺。** crates.io idevice **0.1.68** 自帶
完整 RemotePairing 客戶端（`remote_pairing` 模組）、CDTunnel、
jktcp 用戶態 TCP 棧與 RSD 會話（`rsd`／`RsdHandshake`／
`RsdService`），且與 Swift idevice 後端所綁的 IDevice
v0.1.68-ss 是同一代碼線（版本號同為 0.1.68）。mm-rp 以
**改名相依 `idevice68`**（`package = "idevice"`）引入，與
既有 0.1.29 並存：0.1.29 繼續供 gw-idevice lockdown 路徑與
legacy 根 crate（M3/M5 已驗面不動），0.1.68 只在 mm-rp 與
mm-gateway 的 RP 路徑內使用，其型別不越過 mm-rp 的封裝面。
全協議自研（路 b）不採：SRP/TLV8/TLS-PSK/CDTunnel/RemoteXPC
五層皆已有同作者維護的 Rust 實作，自研只增加漂移面。

## 2. 協議棧逐層（RPK 源碼為規格，括號內為 idevice 0.1.68 對應）

### 2.1 傳輸層（rppairing_transport.cpp）

- TCP 連向裝置 endpoint（RP 服務埠，預設 49152）。
- 訊息分幀：9-byte magic `"RPPairing"` ＋ 2-byte **BE** 長度
  ＋ JSON 本體（上限 0xFFFF）。
- JSON 信封（明文）：`{"message":{"plain":{"_0":<inner>}},
  "originatedBy":"host","sequenceNumber":<n>}`，n 自 0 遞增。
  （0.1.68：`remote_pairing::RpPairingSocket`＋
  `RemotePairingClient.sequence_number`，同形。）

### 2.2 Pair-Verify（rppairing.cpp `rppairing_pair_verify`）

已配對裝置的快速路徑，共 8 步：

1. host 發 handshake 請求：inner＝`{"request":{"_0":
   {"handshake":{"_0":{"hostOptions":{"attemptPairVerify":
   true},"wireProtocolVersion":19}}}}}`
2. host 產臨時 X25519 金鑰對，發 event：`{"event":{"_0":
   {"pairingData":{"_0":{"data":<b64 TLV8>,"kind":
   "verifyManualPairing","startNewSession":true}}}}}`，
   TLV8＝[State=0x01, PublicKey=hostX(32)]。
3. 裝置回 TLV8 [State=0x02, PublicKey=deviceX(32),
   EncryptedData]；含 ErrorResponse(0x07) 即失敗。
4. X25519 DH → shared secret(32)＝**encryption_key 本體**
   （即後續 TLS-PSK 與主密碼種子）。
5. 配對驗證金鑰：HKDF-SHA512(salt="Pair-Verify-Encrypt-Salt",
   IKM=shared, info="Pair-Verify-Encrypt-Info") → 32B。
6. 簽章：Ed25519(identity.private_key) 簽
   `hostX(32) ‖ identifier(UTF-8) ‖ deviceX(32)`。
7. 內層 TLV8 [Identifier, Signature] 以 ChaCha20-Poly1305
   （步驟 5 金鑰、nonce＝`0000‖"PV-Msg03"`）加密，發
   [State=0x03, EncryptedData]（startNewSession=false）。
8. 裝置回 State=0x04；含 ErrorResponse 即失敗。成功後
   衍生主密碼（§2.4）並標記 is_paired。

（0.1.68 對應：`attempt_pair_verify()`＝步驟 1；
`validate_pairing(&mut RpPairingFile)`＝步驟 2–8；
`RemotePairingError::PairVerifyFailed` 對應 ErrorResponse。）

### 2.3 Pair-Setup（rppairing.cpp `rppairing_pair_setup`＋rppairing_srp.cpp）

初次配對（M8 無線配對的主體，此處盤點、M7 不實作）：

1. 發 [Method=0x00, State=0x01]（kind="setupManualPairing"、
   帶 sendingHost）；裝置可能先回 `awaitingUserConsent`
   （信任對話框，再等一幀）或 `pairingRejected`。
2. 裝置回 [Salt(16), PublicKey=B(384)]。
3. SRP-6a（SHA-512、3072-bit N、g=5、k=H(N‖PAD(g))、
   u=H(PAD(A)‖PAD(B))、x=H(s‖H(I‖":"‖P))、I="Pair-Setup"、
   P=PIN（預設 "000000"））；host 發 [State=0x03,
   PublicKey=A（TLV8 於 255B 分片為兩項）, Proof=M1]。
4. 裝置回 Proof=M2，host 驗證之；session key K(64B) 成為
   encryption_key。
5. host **新產** Ed25519 身分（identifier＝UUIDv3(DNS,
   hostname)，見 rppairing_file.cpp）；簽章材料＝
   HKDF(salt="Pair-Setup-Controller-Sign-Salt", info=
   "Pair-Setup-Controller-Sign-Info")；簽
   `signMat(32) ‖ identifier ‖ identity.public_key(32)`；
   加密金鑰＝HKDF(salt="Pair-Setup-Encrypt-Salt", info=
   "Pair-Setup-Encrypt-Info")，nonce＝`0000‖"PS-Msg05"`；
   內層 TLV8 [Identifier, PublicKey, Signature,
   Info=OPACK 裝置資訊字典] 加密後以 State=0x05 發出。
   OPACK（rppairing_opack.cpp）為 Apple 二進位字典編碼；
   RPK 的裝置資訊含 altIRK/btAddr/mac/
   remotepairing_serial_number/accountID/model/name，
   其中 altIRK、mac、序號在 RPK 內是**寫死的假值**
   （0x11 0x22…／"AAAAAAAAAAAA"）——盤點註記，M8 若照搬
   須留意此為 RPK 既有行為而非規格要求。
6. 裝置回 State=0x06 確認；衍生主密碼、標記 is_paired。

（0.1.68 對應：`RemotePairingClient::pair(&mut RpPairingFile,
pin_callback)` 全段；`connect()`＝attempt＋validate 失敗才
pair 的合成路，對應 RPK `rppairing_connect`。）

### 2.4 會話加密與隧道監聽（createListener）

- 主密碼：HKDF-SHA512(salt=空, IKM=encryption_key,
  info="ClientEncrypt-main") 與 info="ServerEncrypt-main"
  各 32B（RPK `derive_main_ciphers`；0.1.68
  `derive_main_ciphers` 同字串）。
- 加密信封：`{"message":{"streamEncrypted":{"_0":"<b64>"}},
  ...}`；ChaCha20-Poly1305 nonce＝encrypted_sequence_number
  的 LE u64 置於 nonce 前 8 byte（餘 0），收發同序號、成功
  後遞增。
- createListener 請求（加密）：`{"request":{"_0":
  {"createListener":{"key":<b64 encryption_key>,
  "transportProtocolType":"tcp"}}}}`；回應（加密）JSON 含
  `"port"`＝裝置側隧道監聽埠。
  （0.1.68：`RemotePairingClient::create_tcp_listener()
  -> u16`。）

### 2.5 TLS-PSK 與 CDTunnel（rppairing_tls_psk.cpp／rppairing_cdtunnel.cpp）

- 新 TCP 連向 endpoint:createListener 埠；TLS 1.2 PSK，
  密碼清單 `PSK-AES256-CBC-SHA384:PSK-AES128-CBC-SHA`，
  PSK＝encryption_key、PSK identity 為空字串（RPK 以
  OpenSSL 實作）。（0.1.68：`tls_psk.rs` 純 Rust TLS 1.2
  PSK-AES256-CBC-SHA384，`connect_tls_psk_tunnel_native`。）
- CDTunnel 握手：TLS 內送 `"CDTunnel"(8B) ‖ 2B BE 長度 ‖
  {"type":"clientHandshakeRequest","mtu":16000}`；回應同
  幀形，JSON 含 `serverAddress`、`serverRSDPort`、
  `clientParameters{address, netmask, mtu}`。
- 產出 tunnel_info 五欄：client_address／client_netmask／
  server_address／server_rsd_port／mtu（RPK
  `rppairing_tunnel_info_t` 與 0.1.68 `TunnelInfo` 欄位
  逐一同名）。
- 隧道內承載裸 IPv6 封包；RPK 以 reader thread 依本地埠
  demux 至各 VirtualTcpStream。（0.1.68：`CdTunnel`＋
  `jktcp::adapter::Adapter`，`AdapterHandle` 實作
  `RsdProvider`。）

### 2.6 用戶態 TCP（rppairing_tcp.cpp `VirtualTcpStream`）

自帶 TCP 協議棧：MSS 1440、視窗 65535、送視窗上限 1MiB、
RTO 初值 200ms、重傳上限 12 次。服務串流、RSD 連線皆以
虛擬 TCP 連向 server_address:port。（0.1.68 對應＝jktcp，
同作者；mm-rp 不自研此層。）

### 2.7 RSD 會話（rppairing_rsd.cpp `RsdClient`）

- 以虛擬 TCP 連 server_address:server_rsd_port，行
  **RemoteXPC 握手**：HTTP/2 preface＋SETTINGS
  （MAX_CONCURRENT_STREAMS=100、INITIAL_WINDOW_SIZE=
  1048576）＋WINDOW_UPDATE(+983041)；stream 1 開 root、
  stream 3 開 reply；XPC wrapper magic `0x29B00B92`、
  物件 magic `0x42133742` v5；依序送空字典（flags
  ALWAYS_SET）、InitHandshake（stream 3, flags
  0x00400000|0x1）、ChannelInit（stream 1, flags 0x201）、
  裝置握手字典（flags DATA|ALWAYS_SET）：{MessageType:
  "Handshake", MessagingProtocolVersion: 7, UUID: <隨機>,
  Properties: {RemoteXPCVersionFlags: 0x0100000000000006,
  SensitivePropertiesVisible: true}, Services: {}}。
- 裝置回覆的服務目錄：RPK 以**原始位元組啟發式掃描**
  解析（找 "com.apple." 字串＋鄰近 "Port"）——實作弱點，
  盤點註記；0.1.68 `RsdHandshake` 以正規 XPC 解碼產出
  `HashMap<String, RsdService{entitlement, port,
  uses_remote_xpc, features, service_version}>`，移植採
  0.1.68 的解碼（線上等價、結果更完整）。
- 服務埠解析（RPK `RsdClient::get_service_port`，**行為
  規格**，mm-rp 須複刻）：依序 (1) 精確名；(2)
  `<name>.remote.trusted`；(3) `<name>.shim.remote`；
  (4) `<name>.remote`；(5) 前綴相符；(6) 子字串相符；
  皆無→0。Swift limd 的 DeviceService 枚舉把 misagent
  直接寫成 `com.apple.mobile.MCInstall.shim.remote` 即
  此解析的應用面。
- 服務串流：虛擬 TCP 連 server_address:service_port；
  plist 分幀＝4-byte BE 長度＋XML（`rppairing_service_
  stream_send_plist/recv_plist`，recv 上限 10MB）；另有
  send_raw／recv_exact 供 AFC 與 DDI 位元組流。
- **RSDCheckin**：Swift limd `withRSDService` 在每條服務
  串流開通後先送 `{"Label":"SideStore","ProtocolVersion":
  "2","Request":"RSDCheckin"}` 並收兩幀回覆（checkin
  回應＋StartService 回應）才執行業務。0.1.68 的各服務
  `RsdService::from_stream` 內含 `idevice.rsd_checkin()`
  ——gw-idevice 路徑由 crate 代勞；gw-limd 路徑以 mm-rp
  原始串流手行同序列（§5 表 C）。

## 3. 對照表 A：RPK C API → idevice 0.1.68 → mm-rp

| RPK C 函數（Swift limd 呼叫點） | idevice 0.1.68 Rust | mm-rp 封裝 |
|---|---|---|
| `rppairing_identity_from_plist` | `remote_pairing::RpPairingFile::from_bytes` | `RpIdentity::load(bytes)` |
| `rppairing_identity_generate`（M8） | `RpPairingFile::generate(sending_host)` | M8 |
| `rppairing_identity_to_plist`（M8） | `RpPairingFile::to_bytes` | M8 |
| `rppairing_client_new(host, port, hostname)` | `RemotePairingClient::new(RpPairingSocket::new(TcpStream), host)` | `RpClient::connect(endpoint, port, host_name)` |
| `rppairing_pair_verify(client, identity)` | `attempt_pair_verify()` ＋ `validate_pairing(&mut file)` | `RpClient::pair_verify(&identity)`（僅 verify、不 fallback setup） |
| `rppairing_pair_setup`（M8） | `pair(&mut file, pin_cb)` | M8 |
| `rppairing_get_encryption_key` | `RemotePairingClient::encryption_key()` | `RpClient::encryption_key()` |
| `rppairing_create_tunnel_listener` | `create_tcp_listener()` | `RpClient::create_tunnel_listener()` |
| `rppairing_tunnel_connect(host, port, psk, …)` | `connect_tls_psk_tunnel_native(stream, key)` → `CdTunnel` | `RpTunnel::connect(...)` |
| （隧道 info） | `CdTunnel.info: TunnelInfo` | `RpTunnelInfo`（五欄同名） |
| `rppairing_rsd_connect(tunnel)` | `RsdHandshake::new(adapter.connect(rsd_port))` | `RpSession::rsd_handshake()` |
| `rppairing_rsd_get_service_port` | `RsdHandshake.services` ＋ §2.7 模糊解析複刻 | `RpSession::service_port(name)` |
| `rppairing_connect_service_stream` | `AdapterHandle::connect(port)`（RsdProvider）／`RsdHandshake::connect::<T>()` | `RpSession::open_stream(name)`／`connect_service::<T>()` |
| `rppairing_service_stream_send/recv_plist` | 串流上 4B BE＋XML（mm-rp 助手） | `RpStream::send_plist/recv_plist` |
| `rppairing_service_stream_send_raw/recv_exact` | `AsyncRead/Write` 直用 | `RpStream` 原始讀寫 |
| `rppairing_tunnel_close`／`rsd_free`／`client_free` | Drop | `RpSession` 失效（generation） |

## 4. 對照表 B：Swift IdeviceGateway 的 IDevice FFI → idevice 0.1.68

| Swift FFI 呼叫 | 語意 | idevice 0.1.68 Rust 對應 |
|---|---|---|
| `rp_pairing_file_from_bytes`／`rp_pairing_file_free` | start 時建 RP 配對檔物件 | `RpPairingFile::from_bytes` |
| `tunnel_create_rppairing_with_options(sockaddr, len, hostname=appName, pairingFile, false, nil, nil, &adapter, &handshake)` | ensureRPConnection：pair-verify＋建隧道＋RSD 握手，一次到位；結果快取，invalidate 才重建 | mm-rp `RpSession::establish()`：TcpStream→`RemotePairingClient`（attempt＋validate）→create_tcp_listener→`connect_tls_psk_tunnel_native`→jktcp `Adapter`→`RsdHandshake` |
| `adapter_free`／`rsd_handshake_free` | invalidateConnection | session generation 失效＋Drop |
| `lockdownd_connect_rsd` | getLockdownValue/fetchUDID | `RsdHandshake::connect::<LockdownClient>(&mut adapter_handle)`（from_stream 含 rsd_checkin；RP 路不另行 StartSession，直接 get_value） |
| `misagent_connect_rsd` | profile 安裝/移除/dump | `connect::<MisagentClient>` |
| `installation_proxy_connect_rsd` | install/remove | `connect::<InstallationProxyClient>` |
| `afc_client_connect_rsd` | house-arrest AFC 的底層連線 | `connect::<AfcClient>` |
| `house_arrest_client_connect_rsd` | vend container | `connect::<HouseArrestClient>` |
| `heartbeat_connect_rsd` | performHeartbeat RP 路 | `connect::<HeartbeatClient>` 後 `get_marco(interval)`→`send_polo()`（與 lockdown 路同序列） |
| `debug_proxy_connect_rsd` | debugApp/debugProcess | `connect::<DebugProxyClient>`＋既有 RSP 工具（M5） |
| `image_mounter_connect_rsd`＋`image_mounter_mount_personalized_rsd(mounter, adapter, img, tc, manifest, chipID)` | mountPersonalizedDdi RSD 路（先以 lockdownd RSD 取 UniqueChipID） | `connect::<ImageMounter>` 的 personalized 掛載（crate 內含 chip id 處理） |
| `performWithService` 失敗重試 | connect 失敗→invalidate→重建隧道→重試**一次**；二敗依 isPairingError 分 invalidPairingFile／serviceError | mm-gateway gw-idevice RP 路徑複刻同重試形狀 |
| `pairable_host_prepare/accept_fd`、`rppairing_pair_network`、`rp_pairing_file_generate/to_bytes/write`、`rppairing_peer_device_free` | 無線配對（M8） | `peer_device`／`responder` 模組＋`RpPairingFile::generate/to_bytes/write_to_file`（M8 細化） |

## 5. 對照表 C：Swift limd gateway 的 RP 助手 → mm-rp

| Swift 助手 | 步驟 | mm-rp 對應 |
|---|---|---|
| `withRPClient(body)` | verifyInitialized→取 rpIdentity→`rppairing_client_new(endpoint, getPort(.rppairing), appName)`→`rppairing_pair_verify`→body→free | `RpSession::with_client`（每次新建、用完即棄；**不快取**） |
| `withRPTunnel(body)` | 有快取隧道直接用；否則 cleanupRPTunnel→withRPClient→create_tunnel_listener→get_encryption_key→`rppairing_tunnel_connect`→快取 (tunnel, info) | `RpSession::tunnel()`（快取＋generation 失效；invalidateConnection＝cleanupRPTunnel） |
| `getActiveRSD(tunnel)` | 快取或 `rppairing_rsd_connect` | `RpSession::rsd()` |
| `withRSDService(service, action)` | withRPTunnel→getActiveRSD→get_service_port→connect_service_stream→**RSDCheckin＋收兩幀**→action→close stream | `RpSession::with_service(name, action)`（原始串流版） |
| `rsdSendPlist/rsdRecvPlist` | XML plist 4B BE 分幀；recv 預設逾時 120000ms | `RpStream::send_plist/recv_plist` |

## 6. limd 後端各操作 RP 分支（Swift 逐行結果）

全數經 `withRSDService(<DeviceService>)`；請求/回應皆 plist：

| 操作 | 服務 | RP 行為 |
|---|---|---|
| getLockdownValue | lockdownd | 送 {Label:"SideStore", Request:"GetValue", Key}；取回應 `Value`（字串），缺/型別不符→serviceError |
| fetchUDID | （同上） | syncGetLockdownValue("UniqueDeviceID")，成功更新 cachedUDID；失敗/空→回 cachedUDID，再無→serviceError |
| isDDIMounted | mobileImageMounter | {Command:"LookupImage", ImageType:"Developer"}；回應有 `ImageSignature` 即 true |
| mountDeveloperImage | mobileImageMounter | {ReceiveBytes, ImageSize, ImageType:"Developer"}→須 Status=ReceiveBytesAck→send_raw 全圖→須 Status=Complete→{MountImage, Developer, ImageSignature}（收覆逾時 30s）→須 Complete |
| mountPersonalizedDdi | mobileImageMounter | 同上但 ImageType:"Personalized"，MountImage 帶 ImageSignature=**manifest**、ImageTrustCache=trustcache |
| installProvisioningProfile | misagent（shim.remote 名） | {MessageType:"Install", Profile:<data>}；回應 Status(Int)≠0→serviceError |
| removeProvisioningProfile | misagent | {MessageType:"Remove", ProfileID}；同上 |
| dumpProfiles | misagent | {MessageType:"CopyAll"}（逾時 10s）；取 `ProfileArray` [Data] 逐一寫 `<dumpDir>/Profile_<i>.mobileprovision`，回 dumpDir |
| removeApp | installationProxy | {Command:"Uninstall", ApplicationIdentifier}；回應丟棄（try?，逾時 30s） |
| sendIpaAfc | afc | RSD AFC 封包（M5 `rsd_afc`）：MakeDir "PublicStaging"→FileOpen "PublicStaging/<bundleId>.ipa" mode 3→Write 1MB 分塊（Status≠0 即敗）→FileClose |
| sendAppBundleAfc | afc | 同上逐檔：MakeDir PublicStaging/<bundleId>/<appName> 後逐檔 Open/Write/Close |
| installIpa | installationProxy | {Command:"Install", PackagePath:"PublicStaging/<bundleId>.ipa", ClientOptions:{PackageType:"Developer"}}；循環收覆至 Status=Complete 或 Error |
| installAppBundle | installationProxy | 同上，PackagePath="PublicStaging/<bundleId>/<appName>" |
| wipeContainer | houseArrest | {Command:"VendContainer", Identifier}；回應丟棄（逾時 10s） |
| debugApp／debugProcess | debugserver | **僅連通串流即返回**（無 RSP 命令）——Swift 原樣如此 |
| performHeartbeat | heartbeat | {Command:"Pico"}（逾時 5s）；回應 `Interval`（UInt64 或 Int）→newInterval，缺→原 interval |
| afcListDirectory／afcReadFile／afcGetFileInfo | — | **無 RP 分支**：直走 lockdown withService（RP 模式下實際不可達，見 §8） |
| startWirelessPair／triggerWirelessPair | — | 恆抛 unsupportedOperation，reason＝`startWirelessPair (RemotePairing is not supported on pure Lockdown gateway)`（trigger 同形） |

gw-idevice 各操作 RP 分支則一律經 performWithEitherService
的 connectRP 路（§4 表 B），含：getLockdownValue（RP 路
不 StartSession）、heartbeat（Marco/Polo）、debugProcess
（lockdown 路恆 unsupported 的操作在 RP 路**可用**——
Swift reason 原文 "only supported in.rppairing mode" 的
另一面）、mountPersonalizedDdi（專屬 RSD 路，§4）。

## 7. idevice crate RP 能力審計（證據）

**0.1.29（現行釘版）：無 RP。**
- `src/` 模組表：afc／core_device_proxy／debug_proxy／dvt／
  heartbeat／installation_proxy／lockdown／misagent／
  mobile_image_mounter／pairing_file／provider／
  springboardservices／tcp／tss／tunneld／usbmuxd／xpc。
- 全樹 grep `RsdHandshake|RemotePairing|rp_pairing|
  RsdProvider`：**零命中**。其 `core_device_proxy` 是
  lockdownd 的 CoreDeviceProxy 隧道服務（配對後、走
  lockdown 連線），與 RemotePairing 協議（SRP/pair-verify
  ＋TLS-PSK＋RSD）是不同協議，不可混用。

**0.1.68（crates.io 最新）：全 RP。**
- feature 表：`remote_pairing`（相依 x25519-dalek、
  ed25519-dalek、hkdf、chacha20poly1305、idevice-srp、
  aes/cbc/hmac/sha1/sha2/rsa、xpc）、`rsd`（xpc）、
  `tunnel_tcp_stack`（jktcp）；`full` 含此三者。
- 模組實證（registry 源碼逐讀）：`remote_pairing/`
  {mod.rs（RemotePairingClient：attempt_pair_verify／
  validate_pairing／pair／create_tcp_listener／
  encryption_key）、peer_device.rs、responder.rs、
  rp_pairing_file.rs（generate／from_bytes／to_bytes／
  write_to_file）、tls_psk.rs、tlv.rs、opack.rs、
  tunnel.rs（connect_tls_psk_tunnel_native→CdTunnel＋
  TunnelInfo 五欄）、socket.rs、lockdown.rs}；
  `services/rsd.rs`（RsdHandshake）；`lib.rs` 的
  `RsdService` trait；八個服務皆有 `RsdService` 實作
  （lockdown／misagent／installation_proxy／afc／
  house_arrest／mobile_image_mounter／debug_proxy／
  heartbeat），其 `from_stream` 皆內含 `rsd_checkin()`。
- 協議常數與 RPK 逐項對照一致：`RPPAIRING_MAGIC=
  "RPPairing"`、`WIRE_PROTOCOL_VERSION=19`、HKDF info
  "ClientEncrypt-main"/"ServerEncrypt-main"、CDTunnel
  magic 與 handshake JSON、TunnelInfo 欄位名。
- 版本譜系：Swift idevice 後端綁 IDevice **v0.1.68-ss**
  （SideStore fork）；crates.io 0.1.68 為其上游同號版。
  fork 與上游的 RP API 差異未逐項比對（fork 的 FFI 面即
  上游 Rust API 的導出層；以 FFI 呼叫點反查上游 API 已
  全數對上，見 §4）。

**接線策略（裁示執行形）**：mm-rp 以改名相依
`idevice68 = { package="idevice", version="0.1.68",
default-features=false, features=[remote_pairing, rsd,
tunnel_tcp_stack, tcp, 及八服務 feature] }` 引入。
不全域升版 0.1.29→0.1.68 的理由：gw-idevice／
gw-idevice-ops（約 1,600 行）與 legacy 根 crate 皆以
0.1.29 API 撰寫且經 M3/M5 真機前驗收形狀鎖定，0.1.68
API 大改（模組重排至 services/、provider 形狀變動），
全域升版等於重開 M3/M5 驗收面，非 M7 範圍。

## 8. 盤點發現（影響實作裁示者）

1. **Swift limd 的 RP 路徑有先天死結**：`verifyInitialized()`
   要求 `isInitialized && cachedUDID != nil`；但
   `syncStart` 在 .rppairing 模式只設 rpIdentity、
   **不設 cachedUDID**（cachedUDID 僅由 lockdown 配對檔
   或成功的 syncFetchUDID 寫入，而 syncFetchUDID 本人
   先過 verifyInitialized）——RP 模式下 limd 全部操作
   （含 withRPClient）恆抛 notInitialized，雞生蛋死結。
   移植裁示：mm-rp 接線後，gw-limd 的 RP 模式驗證改以
   「isInitialized && rpIdentity 已載入」為準（顯名偏離，
   記入 diff 清單 D-M7-*）；否則整個 limd RP 面是死碼，
   與 M7 驗收（兩 gateway 的 RP 路徑）矛盾。
2. limd 的 AFC 三查詢無 RP 分支（§6）：RP 模式下 Swift
   會走 lockdown withService 而連不上——移植照 Swift
   形狀保留 lockdown 路（RP 模式回連線錯誤），不自創
   RSD 版查詢；rsd_afc 模組在 limd RP 路的實際使用點
   只有 sendIpaAfc／sendAppBundleAfc 兩處（M5 盤點的
   「RSD AFC 全套接線」以此為準修正）。
3. RPK 的 RSD 服務目錄解析是啟發式掃描（§2.7）：移植
   採 idevice 0.1.68 的正規 XPC 解碼；目錄**模糊解析**
   規則（§2.7 六級）屬行為規格，於 mm-rp 複刻。
4. RPK pair-setup 的 OPACK 裝置資訊含寫死假值（§2.3
   步驟 5）：M8 盤點註記，本段不處理。
5. gw-idevice 的 `ensureRPConnection` 成功後快取
   (adapter, handshake)；`performWithService` 每次取用
   快取、失敗才 invalidate＋重建＋重試一次。gw-limd
   的隧道亦快取（activeTunnel/activeRsd），但
   withRPClient（pair-verify）**每次新建**——兩後端的
   快取粒度不同，mm-rp 會話層須同時支援兩種持有形狀
   （session 快取型／client 每次新建型），不可一刀切。

## 9. M8 輸入（本段只盤不做）

- start 路：`pairable_host_prepare(hostName, hostModel,
  false, &handle, &serviceId, &txtData, &txtLen,
  &hostAltIrk[16])` → TCPAcceptor 隨機埠 → onReady
  (serviceID, port, txtRecords[plist 解碼]) → accept →
  `pairable_host_accept_fd(handle, fd, onPin, ctx,
  &peerDevice, &pairedRpf)`。
- trigger 路：`generatePairingFile(hostName)`＝
  `rp_pairing_file_generate`＋`to_bytes` 解 identifier →
  `rppairing_pair_network(sockaddr, hostName, rpf,
  pinCallback(60s 等待), ctx, &peerDevice)`。
- 收尾共用：`finalizeAndSavePairedDevice`＝
  `rp_pairing_file_write(rpf, outPath)`＋`to_bytes` 解
  identifier／alt_irk → PairedDeviceRecord(name, model,
  pairingFilePath, pairingFile)；檔名解析
  `resolveFinalPairingPath`（resolveFileName 優先；否則
  outPath 檔名；再否則 `<name>_<model>` 清理字元＋
  `_rp.plist`，清理後為空用 `rp_pairing_file.plist`）。
- peer device 欄位：name／model／udid／alt_irk[16]。
- 0.1.68 對應：`remote_pairing::responder`（pairable
  host 側）與 `peer_device::PeerDevice` 已存在，M8 細化。
