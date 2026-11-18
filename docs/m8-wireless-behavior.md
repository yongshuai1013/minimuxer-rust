# M8 行為清單：WirelessPairService 全量（子系統 9）

日期：2026-11-18。規格來源（皆 `yongshuai1013/minimuxer`
@714aafa3，逐行讀）：`Sources/Services/WirelessPair
Service.swift`（209 行）、`DeviceGateway/DeviceGateway
API.swift`（WirelessPairAPI＋gateway 簽名＋PairedDevice
Record）、`DeviceGateway/idevice/IdeviceGateway.swift`
的 syncStartWirelessPair／syncTriggerWirelessPair／
generatePairingFile／finalizeAndSavePairedDevice／
resolveFinalPairingPath 與 async 包裝、`DeviceGateway/
libimobiledevice/LibimobiledeviceGateway.swift`（兩函數
恆 unsupported）、`Common/TCPAcceptor.swift`、`Common/
MinimuxerConstants.swift`。Tests 內**無**無線配對測試
（全樹僅 EMProxyTests.swift，已查 recursive tree）。

協議證據：idevice 0.1.68 registry 源碼（remote_pairing/
responder.rs、mod.rs、peer_device.rs、rp_pairing_file.rs）
＋作者 ffi 層（git tag v0.1.68 的 ffi/src/pairing_host.rs、
ffi/src/tunnel_provider.rs 的 rppairing_pair_network）
——Swift idevice 後端呼叫的正是這組 FFI，其函數體是對
同一 Rust API 的薄封裝，語意以 ffi 源碼為準。

標記：＝cargo test 可驗；＝留待整合場。

## A. Service 層（WirelessPairService）

- **W1 start 重入**：已有 activeStartTask 時先 cancel
舊的再開新的（start 槽只有一個）。
- **W2 start 收尾**：gateway 成功→completion(.success
(record))；失敗→completion(.failure(err))；兩路徑皆
先清 start 槽＋stopAdvertising，再交付 completion
（MainActor 序）。
- **W3 onReady→廣告**：gateway onReady(serviceID, port,
txtRecords: [String:String]) → 值轉 Data(utf8) →
startAdvertising：先 stopAdvertising 舊廣告，再以
NetService(domain="", type=_remotepairing-pairable-
host._tcp., name=serviceID, port)＋TXT publish，
然後回呼 onReadyToPair(serviceID, port)。
- **W4 onPin→onPinReceived**：gateway onPin(pin) →
MainActor → onPinReceived?(pin)。PIN 的產生方向以
ffi 實證為準：**host 側隨機產生六碼**（responder
pair_setup 於 M2 前經 pin_callback 送出），App 顯示
之、使用者在裝置上輸入。
- **W5 trigger keyed 任務**：key＝"\(targetIp):
\(targetPort)"；同 key 重入先 cancel 舊任務；不同
key 並行；完成後自註冊表移除該 key。
- **W6 trigger onRequestPin**：gateway 要求 PIN 時，
service 把 submitPin 閉包交給 App 的 onRequestPin；
App 未掛 onRequestPin → 立即 submitPin("000000")。

- **W7 stop()**：cancel start 任務＋全部 trigger 任務
＋stopAdvertising。
- **W8 回呼執行緒**：completion／onPin／onReadyToPair
皆 MainActor 交付。Rust 側對應：sink 呼叫點不保證主
線程，由殼層負責切 MainActor（與既有 on_status 同
慣例，屬殼層段事項）。

## B. Gateway 層（idevice 後端；async 包裝僅 withFFIDispatch 轉 sync 實作）

- **G1 start 序**：pairable_host_prepare(hostName,
hostModel, false) → TCPAcceptor() 隨機埠 → onReady
(serviceID, acceptor.port, txtRecords) → accept()
阻塞等連入 → pairable_host_accept_fd(handle, fd,
onPin) → peer device＋paired rpf → resolveFinal
PairingPath → finalizeAndSavePairedDevice
（initialAltIrk＝hostAltIrk）。
- **G2 prepare 內容**（ffi/src/pairing_host.rs 實證）：
pairing_file＝RpPairingFile::generate(hostName)，
identifier＝UUIDv3(DNS, hostName)＝serviceID；
host_info＝PairableHostInfo::generate(hostName,
hostModel)（隨機 alt_irk[16]、wireProtocolVersion
預設 26、allows_pinless＝第 3 參數 false）；TXT＝
host_info.mdns_txt_records(serviceID)：name、
identifier、authTag＝base64(SipHash-2-4(key=
alt_irk, msg=serviceID) 輸出前 6 byte)、model、
flags="1"、ver="26"、minVer="17"；以 XML plist
字典序列化過界，Swift 側解回 [String:String]；
hostAltIrk＝host_info.alt_irk。
- **G3 TCPAcceptor**：socket(AF_INET, SOCK_STREAM)、
SO_REUSEADDR＋SO_REUSEPORT（Darwin）、bind
INADDR_ANY:0、listen(5)、getsockname 取實際埠、
accept() 阻塞回傳連入 fd、deinit close(serverFd)。

- **G4 start peer 欄位**：peer name/model/udid 皆
「ptr 有效且非空字串才採用」；name/model 缺省回退
呼叫端 hostName/hostModel。
- **G5 start 錯誤形狀**：prepare 失敗→serviceError
"pairable_host_prepare failed"；handle 或 serviceId
為 nil→serviceError "Invalid pairable host handle"；
TCPAcceptor init／accept 失敗→POSIXError 原樣抛；
accept_fd 失敗→serviceError "Pairing failed or
cancelled"（ffi 訊息不採用）；pairedRpf 為 nil→
serviceError "No pairing file returned"。
- **G6 trigger 序**：generatePairingFile(hostName)
→ 端點驗證 → rppairing_pair_network(sockaddr,
hostName, rpf, pinCallback, &peerDevice) → peer
欄位 → resolveFinalPairingPath → finalize
（initialAltIrk＝peer.alt_irk）。端點驗證：targetIp
空或 targetPort＝0 → invalidTargetEndpoint，
reason＝"Target endpoint IP (\(targetIp)) or port
(\(targetPort)) is invalid"。
- **G7 trigger PIN 時序**：pinCallback 僅在裝置要求
PIN 時觸發——裝置先回 awaitingUserConsent 時不觸發
且 PIN 恆 "000000"（idevice68 mod.rs
request_pair_consent 實證：consent 分支預置
"000000"，callback 只在 pin 為 None 時於
init_srp_context 前被呼叫）。觸發後以 semaphore 等
App 回 PIN、上限 **60 秒**；逾時（回 nil）→ ffi
get_pin 以 "000000" 代用續行（非中止）。pair_
network 失敗→serviceError，reason＝ffi 錯誤訊息
原文（訊息缺失時 "Pairing failed"）。
- **G8 trigger peer 欄位**：name/model 以 String
(cString:) **無條件採用**（空字串亦然，與 G4 的
非空檢查不同）；udid ptr 非 nil 即採用；alt_irk
[16] 全拷。
- **G9 generatePairingFile 錯誤形狀**：generate 失敗
→serviceError "Failed to generate pairing file"；
rpf 為 nil→"Generated pairing file is nil"；
to_bytes 失敗→"Failed to serialize pairing file
to bytes"；序列化後解 plist 得 identifier 為空→
"Failed to parse identifier from pairing file"。

- **G10 finalizeAndSavePairedDevice**：rp_pairing_
file_write(rpf, finalPath)，失敗→serviceError
"Failed to write pairing file to path"；rp_
pairing_file_to_bytes，失敗→serviceError "Failed
to serialize paired file"；bytes 解 plist 取
identifier（pairedUdid）與 alt_irk Data→hex
（altIrkHex，初值＝initialAltIrk 的 hex）——兩者
**僅供日誌、不進 record**（fallbackUdid 參數在函
數體內亦未被使用）；PairingFileParser.parse(bytes
的 UTF-8 內容）成功用之，失敗→讀回 disk 檔內容再
parse（此 parse/讀檔錯誤以原錯誤原樣抛，屬
PairingError／IO 域）；回 PairedDeviceRecord
(name: peerName, model: peerModel, pairingFile
Path: finalPath, pairingFile: parsed)。
- **G11 resolveFinalPairingPath**：outDir＝outPath
為已存在目錄→本身；pathExtension 為空→本身（視為
目錄）；有副檔名→deletingLastPathComponent。
finalFileName＝resolveFileName(peerName, peerModel)
優先；否則 outPath 的 lastPathComponent 非空且有
副檔名→用之；否則 "\(peerName)_\(peerModel)" 先把
空白、"/"、":" 換 "_"，再僅保留 alphanumerics＋
"_-"，清理後為空→defaultRPPairingFileName
("rp_pairing_file.plist")，非空→＋rpPairingFile
Suffix ("_rp.plist")。outDir 為空→只回檔名。

- **G12 limd 後端**：startWirelessPair／trigger
WirelessPair 恆抛 LibimobiledeviceGatewayError
(.unsupportedOperation)，reason 逐字＝
"startWirelessPair (RemotePairing is not supported
on pure Lockdown gateway)"／"triggerWirelessPair
(RemotePairing is not supported on pure Lockdown
gateway)"。

## C. 協議層（pair-setup；idevice68 源碼實證）

- **P1 trigger 客戶端**＝RemotePairingClient::
connect：attempt_pair_verify → validate_pairing
失敗才 pair()（SRP-6a pair-setup 全段）；僅 setup
真跑時 paired_peer_device 有值（verify 成功路為
None——但 trigger 每次用全新 generate 的 rpf，
verify 必走 setup 路）。
- **P2 start 受理端**＝PairableHost::accept：
handshake（device 角色回應；對 attemptPairVerify
請求回錯）→ SRP-6a server 形 pair-setup M1–M6；
PIN 於送 M2 前經 pin_callback 送出；成功時把裝置
altIRK 存入 pairing_file.alt_irk、encryption_key
為 SRP session key。
- **P3 配對檔格式**：XML plist{public_key(32B),
private_key(32B), identifier, alt_irk?(16B)}；
finalize 產出檔須可被 M3/M7 的 RpIdentity::load
直接載入。

## D. 廣告層（Bonjour；DESIGN §5.2／邊界例外 E2）

- **B1 生命週期**：onReady 後 publish；start 完成
（成功或失敗）或 stop() 時註銷；同一時間至多一個
（startAdvertising 先停舊）。
- **B2 廣告參數**：type＝REMOTE_PAIRING_PAIRABLE_
HOST_SERVICE_TYPE（"_remotepairing-pairable-host
._tcp."）、domain＝""、name＝serviceID、port＝
acceptor 埠、TXT＝G2 的 txtRecords（dns-sd TXT
位元格式：每筆 1-byte 長度＋"key=value"）。


## E. 真機待驗總表（併入專案真機清單）

1. start 全鏈：裝置在「可配對主機」見廣告→連入→
PIN 顯示於 App、於裝置輸入→配對完成→產出檔可
被 M3 路徑（gateway start）直接載入使用。
2. trigger 全鏈：對廣告中的目標發起→裝置顯示 PIN
→App onRequestPin 輸入→配對完成→產出檔同上。
3. WirelessPairViewModel 零改動走完配對全程（殼
層段，DESIGN M8 驗收原文）。
4. 介面列表（network.activeInterfaces）在配對場
景顯示正確（DESIGN M8 驗收原文，M2 遺留同場）。
