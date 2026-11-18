# M8 逐行對讀差異清單：無線配對（子系統 9）

日期：2026-11-18。規格來源：行為清單
`docs/m8-wireless-behavior.md`（Swift WirelessPair
Service＋兩 gateway 無線段逐行＋TCPAcceptor＋常數）。
範圍：DESIGN §4 M8／§5.2、邊界例外 E2。

## 1. pair-setup 裁示：(a) 接線＋補缺（比照 M7）

**證據**（idevice 0.1.68 源碼逐項，非印象）：

- trigger 客戶端全段在 crate 內：`remote_pairing::
  RemotePairingClient::connect`（attempt_pair_verify
  →validate 失敗才 `pair`）、`pair` 的 SRP-6a 全序
  （request_pair_consent／init_srp_context／save_
  pair_record_on_peer）、PIN callback 觸發條件與
  consent 預置 "000000" 皆與 ffi `rppairing_pair_
  network` 的呼叫形一致——該 ffi 函數體即 `TcpStream
  ::connect → RpPairingSocket::new → RemotePairing
  Client::connect(rpf, get_pin)` 的薄封裝。
- start 受理端全段在 crate 內：`remote_pairing::
  responder::{PairableHost, PairableHostInfo}`；
  ffi `pairable_host_prepare` 函數體即 `RpPairing
  File::generate(name)`＋`PairableHostInfo::generate
  (name, model)`＋`mdns_txt_records(identifier)` 序
  列化，`pairable_host_accept_fd` 即 `RpPairing
  Socket::new_device(dup(fd))`＋`PairableHost::
  accept`。Swift gateway 呼叫的正是這組 ffi。
- 配對檔：`RpPairingFile::{generate, to_bytes,
  from_bytes, write_to_file}` 與 M7 的 RpIdentity
  同源同格式。

**補缺範圍**（crate 沒有、本段實作）：WirelessPair
Service 層（keyed 任務註冊表、廣告生命週期、事件面）、
TCPAcceptor 等價、resolveFinalPairingPath／finalize
的 gateway 形狀、Bonjour 廣告（dns-sd）、PIN 請求/
回應 sink、FFI 面。自研 SRP/TLV8 不採（同 M7 理由：
只增加漂移面）。

**兩後端分工**（行為清單 G12）：Swift limd 的 start/
trigger 是恆 unsupported 的「假雙實作」；Rust 同形
——gw-limd 覆寫為逐字 reason 的 unsupportedOperation，
**不假裝雙實作**；實作只在 gw-idevice（經 mm-rp）。

## 2. Bonjour 廣告：主路線 dns-sd（E2 不啟用，見 D-M8-9 落定後回填）

主路線＝Rust 直呼 dns-sd C API（DNSServiceRegister
＋sockfd 泵），依 DESIGN §5.2。spike 於 mm-rp
`advertise` 模組：Apple 目標真 FFI；非 Apple 目標
回平台 unsupported 錯誤（host stub）。結果與 E2
啟用與否記於 D-M8-9。

## 3. 差異清單（D-M8-*）

- **D-M8-1（pair-setup 引擎來源）**：見 §1 裁示。
  mm-rp 只封裝 prepare/accept/pair_network 的會話
  語意與錯誤映射，密碼學全用 idevice68。
- **D-M8-2（取消語意，顯名偏離）**：Swift 的 start
  任務跑在 withFFIDispatch 的阻塞呼叫上，Task.cancel
  對阻塞中的 accept() 無效——stop() 後，被卡住的
  start 要等到有連入（或永遠）才交付 completion。
  Rust 以 CancelToken 真取消：stop()／重入取消會關
  閉 listener 並 drop 進行中的連線，start 以
  serviceError "Pairing failed or cancelled" 立即
  收尾。理由：INVENTORY 對 stop 的規格即「取消全部
  任務並停廣告」；Swift 形狀是阻塞 FFI 的副作用、
  非規格意圖。trigger 同理（drop 連線 future）。
- **D-M8-3（resolveFileName 過界形狀）**：Swift 為
  start/trigger 的 per-call 閉包；Rust FFI 改經
  sink 物件方法 `resolve_file_name(name, model) ->
  MmResolveOutcome{has_resolver, file_name}`（常駐
  物件承載 per-call 閉包，比照 M2 MmConnection
  Config 先例）。語意不變：has_resolver 時用其回值
  為檔名，否則走 G11 預設命名。
- **D-M8-4（PIN 逾時落點）**：60 秒逾時在 gw-idevice
  的 trigger 內（Swift 同位：gateway 的 ffi callback
  semaphore）；逾時以 "000000" 代用續行（ffi
  get_pin 的 nil→"000000" 實證，行為清單 G7）。
  service/sink 層不設逾時；sink 的 request_pin 只
  回答「有無 App 處理者」，答案經 requestId oneshot
  回送（M2 request_service_port 同形）。
- **D-M8-5（POSIXError 映射）**：TCPAcceptor init／
  accept 在 Swift 原樣抛 POSIXError；Rust 映射為
  MmErrorInfo{domain: "NSPOSIXErrorDomain", code:
  errno 數字字串, reason: io 錯誤訊息}，使殼層可
  重建 POSIXError。
- **D-M8-6（TXT 的 ver 值）**：prepare 的 TXT
  ver="26"（PairableHostInfo 預設 wire_protocol_
  version）與 RP 會話的 WIRE_PROTOCOL_VERSION=19
  是兩個不同的值——ffi 實證如此，原樣保留、不
  「修正」。
- **D-M8-7（onReady 的同步性）**：Swift 在 MainActor
  Task 內啟廣告、與 gateway 的 accept 並行；Rust 的
  events.on_ready 為同步呼叫（dns-sd 註冊本身非阻
  塞、立即返回），在 acceptor 已 bind 後、accept 前
  執行，時序等價（廣告必在 accept 等待期間生效）。
- **D-M8-8（generatePairingFile 的 identifier 讀法）**：
  Swift 自序列化 bytes 解 plist 取 identifier；Rust
  同形（自 RpPairingFile::to_bytes 解 plist dict），
  不直接讀 crate 欄位——保留 G9 的失敗形狀位置，
  雖然 generate 本身無失敗路。
- **D-M8-9（Bonjour spike 結果／E2 啟用與否）**：
  （實作段回填。）
- **D-M8-10（finalize 的死參數）**：Swift finalize
  的 fallbackUdid 參數與 pairedUdid／altIrkHex 計
  算皆不進 record（僅日誌）；Rust finalize 不設
  fallbackUdid 參數、record 形狀一致（name/model/
  path/pairingFile 四欄）。pairingFile 欄位在 Rust
  為 mm_common::PairingFile（Swift 為 `any
  PairingFile` 協議），過 FFI 時以序列化內容字串
  交付、殼側以 mm_parse_pairing_file 重建。
