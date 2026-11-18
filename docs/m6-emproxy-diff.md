# M6 逐行對讀差異清單：EMProxy 整合（子系統 8）

日期：2026-11-18。規格來源：行為清單 `docs/m6-emproxy-behavior.md`
（EMProxyTests.swift／EMProxyImpl.swift／EMProxyAPI／App 側
EMProxyWrapper＋AppBootManager 逐行轉錄）。範圍：DESIGN §4 M6。
本文件先記 M0(b)／風險 1 的接入裁示與證據，再記實作對照與 D 表。

## 1. 接入裁示（M0(b)／風險 1）：crate 相依（首選路成立）

**裁示：路 1 — crate 相依。** mm-emproxy 以 git 相依釘
`SideStore/em_proxy` rev `6e117e140ca7cff4ff106bdefa18147552a0e592`
（tag v0.9.3＝develop HEAD；即 Swift Package.swift binaryTarget
所釘 xcframework 的同一份源碼，release checksum 3998789c…），
經其公開 Rust API `start_loopback(SocketAddrV4) ->
Result<ProxyHandle, c_int>` 自行持有 handle（sender＋join_handle），
日誌以其 `set_log_callback` 註冊轉發。路 2（xcframework 連結）
不啟用；路 3（重寫）依設計拒絕、未評估。

證據：
- em_proxy 的 Cargo.toml 宣告 `[lib] crate-type = ["staticlib",
  "rlib"]`——rlib 在場，可作一般 crate 相依；且有公開 Rust 面
  （`start_loopback`、`set_log_callback`、型別 `ProxyHandle`），
  不必經 C 字串層呼叫。
- 金鑰以 `include_str!("../keys/…")` 相對 crate 源碼目錄嵌入；
  git checkout 後路徑在其 checkout 內自洽（host 建置已實證，
  伺服器實測見行為清單 C 組測試）。
- 本機實證：`cargo build -p mm-emproxy`（host）全綠（boringtun
  0.7.1 等相依編譯通過）。iOS 三 target 的本機 check 被**環境**
  擋下：boringtun 的 ring 0.17.14 build script 需 clang/xcrun
  （Apple SDK 工具鏈），本 Linux 主機無此工具——非 crate 路的
  阻礙證據；權威驗證在 macOS CI（em_proxy 自家 CI 亦以同一
  crate 在 macOS 建 aarch64-apple-ios/-sim 的出貨 xcframework，
  justfile 的 apple-build 即此路）。
- 鎖檔連帶變更（僅版本解算，無 API 衝突）：portable-atomic
  1.11.0→1.15.0（boringtun 需 ^1.13.1、jiff 需 ^1.10，交集成立）、
  libc 0.2.171→0.2.191；新增 boringtun 0.7.1、x25519-dalek
  2.0.1、blake2、chacha20poly1305 等（boringtun 相依樹）。
- 路 2 的已知劣勢（備查）：需在 CI 下載並釘 release zip、
  雙 staticlib 併入單一 xcframework 的組裝成本、且其隨附頭檔
  `include/EMProxy/em_proxy.h` 是**陳舊檔**——宣告
  `test_emotional_damage(int)`，v0.9.3 lib.rs 無此符號（行為
  清單 C7），以頭檔為準的封裝會踩空。

## 2. 實作對照（mm-emproxy ← EMProxyImpl）

| Swift | Rust | 備註 |
|---|---|---|
| `EMProxyImpl`（facade 的 `emproxy` 實例，MinimuxerImpl 持有） | `mm_emproxy::EmProxy`（Mutex 內部狀態）；mm-core 持有、FFI 經 core 取得 | 單例形狀對齊 em_proxy 的程序級 GLOBAL_HANDLE |
| `setHandshakeClient(host, port, enabled)` | `EmProxy::set_handshake_client(&str, u16, bool)` | NSLock → Mutex；兩參便捷版在殼層（Swift extension），FFI 面只開三參原形＋殼側預設 |
| `start(host, port)` | `EmProxy::start(&str, u16) async` | 順序：查 config（無→handshakeClientNotConfigured）→ 組 `"host:port"` → 解析 SocketAddrV4（敗→invalidSocketAddress(組串)）→ `em_proxy::start_loopback`（-4/-5 經映射）→ 握手探測（不抛） |
| C `start_emotional_damage` 的 -1/-2（null/UTF-8） | 不可達（Rust `&str` 面） | 映射函數 `map_start_code` 保留全表供邊界與測試（行為 A4） |
| 已在跑再 start → C 回 0 | `start` 見 handle 在場 → 直接 Ok（並記錄） | A5；探測仍照常執行（Swift 亦在 C 回 0 後續行探測） |
| `stop()` | `EmProxy::stop()`（同步；async 形狀在 FFI 面，同 Swift 的 withFFIDispatch 跳板） | handle 取出 → sender.send（敗→stopSignalFailed，優先於 join 結果，同 C 的 match 順序）→ join（敗→threadJoinFailed）；無 handle→serverNotRunning |
| init 註冊 log 回呼 | 首次建構時 `register_log_callback_once`（Once） | 轉發規則：前綴 `[EMProxy] `、level≤1→verbose、≥2→debug（行為 A8）；em_proxy 以 CString 呼叫、回 true 消費 |
| `triggerVPNHandshake` | `probe_trigger` | 200ms 間隔、總時長、1s 單次上限皆 ProbeParams（生產＝Swift 常數；測試注入短化） |
| `probeVPNHandshake` | `probe_candidates` | 候選源注入（見 §3 接線）；並行探全部候選、首勝即輪勝（tokio JoinSet，勝出即 abort 其餘——Swift TaskGroup 是等全組回報後 cancelAll，首勝語義同、時延差異見 D-M6-3） |
| `probeHost`＋`isProbeSuccessful` | `probe_host`（mm-common `probe_tcp_outcome`） | Connected/Refused→true；Failed→false（行為 B4） |
| `discoverUtunCandidateDestinations` | 候選 provider 閉包 | 生產接線在 mm-core：`mm_net::scan(true)` → `resolve_candidate_peers` → ip 列表（每次探測呼叫時新掃，同 Swift） |

## 3. 接線（mm-core／mm-ffi）

- DESIGN §2 的 crate 圖：mm-core 相依 mm-emproxy（「EMProxy
  調度」）。`MinimuxerCore` 新增 `emproxy: EmProxy` 欄位（建構
  時以 mm-net 候選 provider 組裝），`pub fn emproxy(&self)` 暴露
  ——對應 Swift MinimuxerImpl 持有 emproxy 實例、facade
  `Minimuxer.emproxy` 暴露同一顆。
- core 的 start/stop **不**啟停 EMProxy（Swift MinimuxerImpl
  全文核實：僅 init 存放、start/stop 未呼叫）。App 順序
  （AppBootManager：enableEMPforWireguard → startEMProxy 直抛
  中止 → minimuxerStart）由殼層以 FFI 面順序複刻，屬整合段
  收編；本段只備齊 bridge 面（行為 E1/E2）。
- FFI 面（mm-ffi）：`mm_emproxy_set_handshake_client(host, port,
  enabled)`、`mm_emproxy_start(host, port)`（async）、
  `mm_emproxy_start_default()`（async；EMP_SERVER_HOST/PORT）、
  `mm_emproxy_stop()`（async）、`mm_emproxy_is_running()`
  （FFI-only 查詢工具，同 mm_version 定位）。錯誤一律
  MmOutcome＋EMProxyError 的 MmErrorInfo（mm-common 既有型別）。
- CI：test job 的 cargo test 清單與 iOS check 清單加入
  `-p mm-emproxy`（xcframework 經 mm-ffi 傳遞相依自動含入）。

## 4. D 表（M6 顯名分歧與已知損失）

- **D-M6-1**：start 的 -1（invalidBindAddressPointer）／-2
  （invalidUTF8String）在 Rust 面不可達——它們是 C 字串指標層
  的錯誤，mm-emproxy 的 API 收 `&str`。EMProxyError 十例與映射
  表全數保留（`map_start_code`），殼層若日後經 C 面呼叫仍可
  對映；A4 以映射表測試覆蓋此二碼。
- **D-M6-2**：-5（cryptoInitFailed）以出貨金鑰不可達：金鑰是
  em_proxy 編譯期嵌入的 repo keys/，解析失敗只會發生在上游
  改版弄壞金鑰檔時。映射保留；無 host 觸發路徑。
- **D-M6-3**：probe 並行輪的時延差異：Swift TaskGroup 等該輪
  **全部**候選回報（或各自 1s 上限）才判定輪敗、再睡 200ms；
  Rust 以 JoinSet 首勝即 abort 其餘——輪勝判定同，但輪敗的
  最長耗時同為單次上限（1s），節奏等價；唯一差別是敗輪中
  慢候選被提前取消，不影響結果語義。
- **D-M6-4**：probeHost 的 `.waiting(其他錯誤)→續等` 在 BSD
  複刻下塌縮為 Failed（SO_ERROR 非 0 非 ECONNREFUSED 即該次
  失敗）：NWConnection 的 waiting 中間態在 BSD connect+poll
  無對應可觀測態；後果僅是該輪提早判敗、由外層 200ms 重試
  吸收（DESIGN M6 指定以 errno 判讀複刻，已知取捨）。
- **D-M6-5**：log 回呼的層級對映照 Swift 原碼：em_proxy level
  ≤1（DEBUG/INFO）→verbose、≥2（WARN/ERROR）→debug——注意這
  與直覺相反（是 EMProxyImpl 原碼如此，逐字複刻，不「修正」）。
- **D-M6-6**：`test_emotional_damage` 不移植（行為 C7：頭檔
  殘留、v0.9.3 無符號、Swift 無呼叫點）。
- **D-M6-7**：mm-emproxy 新增對 mm-net 的**運行期**接線需求
  （候選 provider）以閉包注入解決，未新增 crate 相依邊
  （DESIGN §2 的 crate 圖不變：mm-emproxy 只相依 em_proxy、
  mm-common、tokio）；provider 的組裝在 mm-core。
- **D-M6-8**：C 層的跨實例全域單例（GLOBAL_HANDLE）不複刻。
  Swift 側 EMProxyImpl 亦僅由 Minimuxer 單例持有一份，
  App 可達情境下「每 MinimuxerCore 一個 EmProxy、handle
  實例持有」與全域單例語義等價；第二個 Minimuxer 實例同存
  的邊界情境 App 不可達（Minimuxer.shared 單例）。

## 5. 測試轉寫事故與本機證據鏈（2026-11-18）

- **INITIAL_CHAIN_KEY 轉寫事故**：WG 測試常數初版把
  initialChainKey 寫錯（第 4 位元組起全錯），伺服器回
  `Decapsulation error: InvalidAeadTag`。逐層對版後定讞：
  Swift 原文（EMProxyTests.swift:252-255）、boringtun 0.7.1
  常數、BLAKE2s("Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s")
  真值三方一致，錯在本 port 的轉寫。教訓：常數一律自源碼
  檔逐字節複製，不經二手摘要。修正後中間值（eph_pub／dh1／
  ck／key1／hash／client_pub／dh2）與獨立 Python 實作全數
  一致、enc_static 逐字節一致。
- **本機沙箱 UDP EPERM**：見行為清單 §F。本機 C 組證據鏈：
  (1) initiation 封包與獨立 Python（RFC 8439 向量驗過的
  ChaChaPoly＋RFC 7748 ladder＋hashlib BLAKE2s）逐字節一致；
  (2) 同封包餵獨立 boringtun 0.7.1 Tunn（同金鑰）回 92B
  handshake response；(3) 經 em_proxy 實跑，伺服器日誌自
  「Decapsulation error」轉為「Error sending UDP packet:
  EPERM」——即 boringtun 已產出 response、僅送出被沙箱擋。
  C1/C2/C4/C5 全斷言待 macOS CI 裁決（workflow 的 cargo
  test 步）。
- **CI 裁決（run 44245093690，HEAD c20564ab）**：macOS 上
  wireguard_server 5/5 全過——**C1 握手、C4 內層 IPv4
  loopback 交換皆全斷言通過**（boringtun allowed-IP 未擋，
  無須降級真機）；config_and_state 13/13、handshake_probe
  6/6、mm-ffi 24/24（含 ffi_emproxy_face_shapes）、mm-core
  55、mm-common 21。attempt 1 僅敗於 mm-gateway 既有 flaky
  `open_service_connects_to_started_port`（mock 埠時序，
  M5 首見、本次第 2 次再現；同 run attempt 2 通過，依派工
  紀律記錄不改判）。attempt 2 全綠，artifact
  MinimuxerRust.xcframework（14401794538）驗包通過：
  sha256 50e0fb6d… 與內附 .sha256 一致、device slice
  arm64（cputype 16777228）、綁定含 mm_emproxy 面、
  device .a 抽驗含 EmProxy／mm_emproxy_stop 符號。
