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

## 2. Bonjour 廣告：主路線 dns-sd 成立，E2 不啟用（D-M8-9 已落定）

主路線＝Rust 直呼 dns-sd C API（DNSServiceRegister
＋sockfd 泵），依 DESIGN §5.2。spike 於 mm-rp
`advertise` 模組：Apple 目標真 FFI；非 Apple 目標
回平台 unsupported 錯誤（host stub）。macOS CI
（run 44262219678）spike 測試真註冊通過，主路線
成立、**E2 後備殼不啟用**，證據見 D-M8-9。

註記：首輪 CI（runs 44260532016／44261081707／
44261453887）紅在 host 測試步——advertise 的
Apple 專屬模組有一處型別錯誤（RegisterReply 為
Option<fn>、呼叫點傳裸 fn），本機 Linux 編不到
該模組而未攔下；修復 commit 85b04e04（呼叫點改
Some(register_reply)，並以獨立 rustc 對該模組做
型別檢查確認無其他同類問題）後全綠。

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
  **主路線成立、E2 不啟用。** 證據：macOS CI
  run 44262219678 的 host 測試 job（134658049203）
  全綠，其 mm-rp advertise 測試組 4/4 含
  `spike_register_pairable_host_service ... ok`
  ——在 CI 的 macOS 上以正式 service type
  `_remotepairing-pairable-host._tcp.`＋完整 TXT
  真註冊、callback 回 0、stop 後註銷乾淨。同一
  run 的 xcframework artifact 14409069689（47,433,
  486 B）驗包通過：zip sha256 與附帶 .sha256 一
  致、device＋simulator 雙 slice、device .a 含
  mm_wireless_* 七導出與 DNSServiceRegister 參
  照、舊符號連續。真機上廣告是否被裝置端看見
  仍屬真機待驗（行為清單 E 表）。
- **D-M8-10（finalize 的死參數）**：Swift finalize
  的 fallbackUdid 參數與 pairedUdid／altIrkHex 計
  算皆不進 record（僅日誌）；Rust finalize 不設
  fallbackUdid 參數、record 形狀一致（name/model/
  path/pairingFile 四欄）。pairingFile 欄位在 Rust
  為 mm_common::PairingFile（Swift 為 `any
  PairingFile` 協議），過 FFI 時以序列化內容字串
  交付、殼側以 mm_parse_pairing_file 重建。
- **D-M8-11（任務槽清除的 id 比對，簿記硬化）**：
  Swift 的 start/trigger completion 無條件清槽
  （activeStartTask = nil／removeValue(forKey:)）
  ——重入時舊任務的遲到 completion 會誤清新任務
  的槽（新任務仍跑、但 stop() 再也取消不到）。
  Rust 改為「id 相符才清」，stop() 覆蓋面完整；
  App 可見行為（completion 交付、取消傳播）不
  變，僅簿記差異。
- **D-M8-12（註冊表落點與 onReady 三元組）**：
  DESIGN §5.2 文字把 keyed 任務註冊表寫在 mm-rp
  內；實際落點為 mm-core 的 WirelessPairService
  ——因註冊表須同時協調 gateway 操作、廣告
  （mm-rp）與 App 事件，且 Swift 原物即位於
  facade 層（MinimuxerImpl.wirelessPair），mm-rp
  只持會話／廣告原語。另 §5.2「onReady 三元組原
  樣過界」：(serviceID, port, txtRecords) 在
  gateway→service 界面原樣傳遞；對 App 的 sink
  只出 (serviceID, port)，與 Swift 的
  onReadyToPair(String, Int) 逐字一致（TXT 在
  Swift 亦不達 App、僅供 startAdvertising）。
