# M9 整合檢查清單（殼層＋SideStore 切換段任務書底稿）

日期：2026-11-18。輸入：`docs/m9-equivalence-audit.md`（等價審計）、
mm-ffi 現況（rust-rewrite @M9）。本清單寫到可直接照做：下一段照
此逐步執行，每步有驗證點。本段未動 SideStore 倉庫、未寫殼。

## 1. FFI 導出全表（mm-ffi 現況，殼必須全數有對應面）

共 **81 個** swift-bridge 導出（`crates/mm-ffi/src/lib.rs` 的
`#[swift_bridge::bridge]` 模組），綁定碼已入庫於
`crates/mm-ffi/generated/`（minimuxer_rust.swift/.h、
SwiftBridgeCore.swift/.h）——殼直接引用，不需 Rust 工具鏈重生；
若下一段改 FFI 面，須同步重生綁定並入庫（CI 的一致性檢查點，
見 §4）。

- 基礎與 sink（10）：mm_version、mm_parse_pairing_file、
  mm_describe_error、mm_test_tcp、mm_async_ping、mm_emit_log、
  mm_emit_status、mm_register_event_sink、mm_clear_event_sink、
  mm_usbmuxd_retarget
- mm-net（11）：mm_net_scan_interfaces、mm_net_snapshot、
  mm_net_endpoint、mm_net_connection_mode、mm_net_refresh、
  mm_net_observer_start、mm_net_observer_stop、
  mm_net_bind_connection_config、mm_net_clear_connection_config、
  mm_net_set_probe_timeout、mm_net_complete_service_port
- mm-gateway hub（15）：mm_gateway_backend、mm_gateway_set_backend、
  mm_gateway_requires_usbmuxd、mm_gateway_start、mm_gateway_stop、
  mm_gateway_set_endpoint_ip、mm_gateway_pairing_protocol、
  mm_gateway_service_port、mm_gateway_set_port、
  mm_gateway_fetch_udid、mm_gateway_get_lockdown_value、
  mm_gateway_start_session、mm_gateway_stop_session、
  mm_gateway_open_service、mm_gateway_set_logging、
  mm_usbmuxd_retarget
- usbmuxd 代理（5）：mm_proxy_start、mm_proxy_stop、
  mm_proxy_is_listening、mm_proxy_notify_attached、
  mm_proxy_notify_detached
- mm-core（28）：mm_core_start、mm_core_stop、mm_core_restart、
  mm_core_reinitialize_pairing_data、mm_core_bind_connection_config、
  mm_core_is_ready、mm_core_fetch_udid、mm_core_is_ddi_mounted、
  mm_core_mount_ddi、mm_core_send_ipa_afc、
  mm_core_send_app_bundle_afc、mm_core_install_ipa、
  mm_core_install_app_bundle、mm_core_remove_app、
  mm_core_wipe_container、mm_core_debug_app、
  mm_core_attach_debugger、mm_core_install_profile、
  mm_core_remove_profile、mm_core_dump_profiles、
  mm_core_afc_list_directory、mm_core_afc_read_file、
  mm_core_afc_get_file_info、mm_core_set_params、mm_core_status、
  mm_core_set_logging、mm_core_is_pairing_file_loaded、
  mm_core_last_beat_successful
- mm-emproxy（5）：mm_emproxy_set_handshake_client、
  mm_emproxy_start、mm_emproxy_start_default、mm_emproxy_stop、
  mm_emproxy_is_running
- 無線配對（7）：mm_wireless_bind_sink、mm_wireless_clear_sink、
  mm_wireless_start、mm_wireless_trigger、mm_wireless_stop、
  mm_wireless_complete_pin、mm_wireless_state

## 2. Sink／回呼形狀（殼必須實作的三個 Swift 物件）

1. **MmEventSink**（`mm_register_event_sink` 註冊、單一常駐）：
   - `on_log(message: String)`——已含 `[debug]`/`[verbose]` 前綴標籤。
   - `on_status(seq: UInt64, success: Bool, domain: String,
     code: String, reason: String)`——seq 單調遞增，殼側以
     PassthroughSubject 重播為 statusPublisher；success=false 且
     domain/code 非空時以 MmErrorInfo 重建 MinimuxerError 回
     `.failure`，空時回 `.success(true)`（D-M4-3 的 Bool 限制在此
     落地：`.success(false)` 不可達推播面，殼不需處理）。
   - 注意：註冊 sink 會同時開啟 Rust 側 logging 路由
     （mm_register_event_sink 內 set_logging_enabled(true)）；
     殼若要預設關閉，註冊後再呼叫 mm_core_set_logging(false)。
2. **MmConnectionConfig**（`mm_core_bind_connection_config` 傳入）：
   11 方法——set_tunnel_iface_ip、set_tunnel_peer_ip、
   set_tunnel_peer_subnet_mask、set_tunnel_peer_reachable、
   set_tunnel_iface_subnet_mask、set_remote_reachable、
   set_override_tunnel_peer_reachable（7 setter，Option<String>
   以空字串為 nil 哨兵，殼讀 "" 轉 nil）；get_connection_mode、
   get_override_tunnel_peer_ip、get_remote_server_ip（3 getter）；
   request_service_port(current) → 殼以 Bonjour 探索回新埠，
   再以 `mm_net_complete_service_port(request_id, port)` 回送
   （請求/回應形，仿 M2 先例）。
3. **MmWirelessPairSink**（`mm_wireless_bind_sink` 註冊）：
   - `on_pin_received(pin)`、`on_ready_to_pair(service_id, port)`。
   - `request_pin(request_id) -> Bool`——回「App 有無處理者」；
     PIN 本體經 `mm_wireless_complete_pin(request_id, pin)` 回送；
     無人回應時 gateway 60 秒逾時以 "000000" 代用（D-M8-4）。
   - `resolve_file_name(name, model) -> MmResolveOutcome
     {has_resolver, file_name}`（D-M8-3）。

## 3. 錯誤轉換表產生方式（error-map.toml → Swift）

- 真值：`crates/mm-common/error-map.toml`（73 例，見審計 §8）。
- 產生器：殼層段寫一個小腳本（Python 讀 toml）產出 Swift 表：
  `(domain, code) → MinimuxerError/PairingError/EMProxyError/
  DeviceGatewayError 的 case 建構`，含 payload 解碼規則
  （toml 檔頭註解：invalidPairing/mount 的 reason 以 U+0001 分隔
  `<protocol>\u{1}<reason>`；PairingError.incomplete 同形帶缺鍵
  清單；EMProxyError.unknownError 的 reason 為 i32 字串）。
- 落點：殼套件內 `MmErrorInfo+Swift.swift`（名稱下一段定），
  由腳本產出後入庫，檔頭註明「generated from error-map.toml,
  do not edit」；CI 加一步重跑產生器並 diff（漂移即紅）。
- describe：錯誤顯示文字直接呼叫 `mm_describe_error`（Rust 側
  模板渲染），殼不另寫 description 表，避免雙真值。

## 4. 子模組切換與 Package 殼步驟（DESIGN §6 SideStore 接入清單）

1. SideStore 開分支 `rust-minimuxer`（自 develop）。
2. `.gitmodules`：`Dependencies/minimuxer` 的 url 改指
   `yongshuai1013/minimuxer-rust`、branch 改 `rust-rewrite`
   （或釘版 commit），path 不變；子模組更新到 M9 結案樹。
3. 新 Package.swift 殼（置 minimuxer-rust 倉根或套件目錄，下一段
   定）：產品/模組名與 Swift 版同名同三產品面
   （Minimuxer／MinimuxerCommon／DeviceGateway 的 App 可見面），
   以 binaryTarget 引用 MinimuxerRust.xcframework（Release 資產
   ＋checksum 釘版，比照 EMProxy 既有模式）或本地 path（開發覆寫
   腳本 `Scripts/use-local-xcframework.sh`，不入庫）。
4. 殼實作面：MinimuxerAPI 相容層（MinimuxerWrapper 零改動為
   驗收線）——facade 五面、MinimuxerParams/set、statusPublisher
   （sink→PassthroughSubject）、三個 sink 物件（§2）、錯誤轉換
   表（§3）。pathPublisher 保留屬性但不發射（審計 D7 豁免），
   殼註解顯名。
5. Swift 版套件依賴自 SideStore Package 樹移除（原 minimuxer
   子模組的 Swift 產品不再被引用後，檢查 project.yml 零改動
   原則：僅允許套件引用行變化，不動 App target）。
6. EMProxy binaryTarget 移除（功能已由 mm-emproxy 內含 em_proxy
   crate 取代）；App 的 EMProxyWrapper 改指殼面
   （setHandshakeClient/start/stop 三面，審計 H1–H4）。

## 5. CI 轉正檢查點

- minimuxer-rust 倉：`build-xcframework.yml` 保持——workspace
  check（含 limd feature）、新 crates host test、iOS check、
  xcframework 雙 slice 建置；綁定碼入庫一致性（重生後 diff 為空）。
- Release：tag `rust-v<semver>` 產 xcframework zip＋sha256；
  Package.swift 的 binaryTarget url/checksum 與 Release 同版
  （不一致即紅，比照 DESIGN §6.5）。
- SideStore 倉：develop CI 在 `rust-minimuxer` 分支綠（checkout
  submodules 步驟不變、不新增 Rust 工具鏈步驟——綁定入庫＋
  xcframework 遠端引用保證純 Xcode 構建）。
- 轉正：分支合入 develop 的條件＝§6 真機清單全過＋IPA 驗包過。

## 6. IPA 驗包要點

- 版本字串：IPA 內嵌 xcframework 的版本與子模組釘版、Release
  tag 三者一致（strings 抽驗 `mm_version` 回值面）。
- 簽名：App 簽名與內嵌 framework 簽名鏈完整（codesign --verify）。
- 符號連續性：device slice .a 含 M1–M9 全導出符號抽驗
  （mm_core_is_ready、mm_emproxy_start、mm_wireless_start、
  mm_gateway_fetch_udid 等哨兵符號），與上一版 artifact 對照
  無消失。
- 體積對照：與 Swift 版 IPA 比對異常膨脹（雙 idevice 版本並存
  D-M7-2 的體積成本在此量化）。

## 7. 真機全清單（M2–M8 各段待驗合併去重，逐項標來源）

整合場（SideStore rust-minimuxer 分支 IPA）一次跑完：

1. **狀態點三態**：隧道開/關/重開，紅綠與 Swift 版同色同時序
   （M4）；**E1 判準**：隧道切換後 ≤2 秒 endpoint 更新
   （輪詢理論上限約 1.15 秒；不合格才啟用 E1 殼）（M2）。
2. **裝置註冊流程**以 Rust 核心通過（M4）。
3. **BrokenPipe/early eof A/B**：同裝置同隧道，Swift 版 vs
   Rust 版各 30 分鐘計斷線次數（M4／DESIGN 風險 8）。
4. **連線設定頁**「從網路自動探索」欄位與 Swift 版同場景
   一致；**漂移定讞場**：同場景抓路由狀態＋勝出候選日誌
   （審計 §10 儀器），判 10.7.0.1/32↔10.7.0.0/24 來源（M2/M9）。
5. **兩後端 fetchUDID 同值**、與 Swift 版同值；limd 經代理
   ListDevices 正常（M3）。
6. **真 IPA 安裝一輪**：send→install→remove 全通、兩後端同
   結果；dump profiles zip 解壓與 Swift 版逐檔一致；JIT
   debugApp 能 attach（M5）。
7. **DDI 掛載兩路**：pre-17 與 personalized 各一場；limd
   personalized 的 mux pair record 供應是否成立（D-M5-7
   判別點）；AFC 單次 Write 大 IPA 上限觀察（D-M5-15）（M5）。
8. **AppBootManager 冷啟鏈**：startEMProxy→minimuxer start→
   fetchUDID 驗連；真 WG client 全鏈（dummy listener 場景）；
   EMProxy 候選探測真機值與 Swift 版一致（M6 D1/D2/D3）。
9. **rppairing 模式全場**：RP 配對檔 isReady 綠、fetchUDID、
   心跳存活 10 分鐘、RP 安裝一 App；兩後端 RP 同場對照
   （limd 基準為行為規格，D-M7-1）；misagent RSD 目錄實名
   （D-M7-10）；debugApp/debugProcess RP 路；personalized
   DDI RSD 路；RP 錯誤 reason 碼源差異（D-M7-6）日誌比對（M7）。
10. **無線配對全程**：WirelessPairViewModel 零改動走完
    start（廣告被裝置看見、PIN、產檔）與 trigger（PIN 請求/
    回應、60 秒代用）；產出配對檔可直接被 lockdown/RP 路徑
    載入使用；activeInterfaces 列表正確（M8）。
11. **登入後刷新一輪**（DESIGN M9 驗收）：刷新、安裝、移除、
    JIT、無線配對、rppairing 模式、狀態點三態——與第 1/6/9/10
    項同場覆蓋，逐項打勾後才合 develop。

## 8. 下一段開工前置確認

- [ ] 本清單與審計檔已入庫（M9 結案 commit）。
- [ ] xcframework Release 已發（tag＋checksum）或決定先以本地
      path 覆寫開發、Release 後補。
- [ ] SideStore `rust-minimuxer` 分支已開、子模組釘版已定。
