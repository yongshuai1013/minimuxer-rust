# M5 逐行對讀差異清單：安裝管線＋DDI 掛載本體＋debugserver 基本面

日期：2026-11-18。規格來源：`yongshuai1013/minimuxer` @714aafa3 之
`DeviceGateway/DeviceGatewayAPI.swift`、`BaseDeviceGateway.swift`（RSP 工具段）、
`idevice/IdeviceGateway.swift`、`libimobiledevice/LibimobiledeviceGateway.swift`、
`Sources/Services/Mounter.swift`、`Sources/MinimuxerImpl.swift`（操作 forwards、
ensureDDIMounted、dumpProfiles、mountDDI 段）。
範圍：DESIGN §4 M5——子系統 11 操作面（AFC／instproxy／misagent／house-arrest／
debugserver）＋子系統 10（Mounter）。兩後端的 RSD（rppairing）路徑屬 M7：本段只做
lockdown 路徑，RP 分支一律回 `unsupportedOperation`（比照 M3／M4 慣例）；
唯 RSD AFC 封包編解碼模組依 DESIGN M5 驗收條目在本段落地（§6，未接線）。
D-M4-4（M4 的 mountDDI 僅任務管理、本體回 unsupported）在本段結案（§5）。

底層複用出處：`idevice` crate 0.1.29（MisagentClient／ImageMounter／
InstallationProxyClient::get_apps／LockdownClient）、rusty_libimobiledevice
（SideStore fork，feature `limd`）、舊 minimuxer crate 的 wrapper 用法實績
（`src/install.rs`／`provision.rs`／`mounter.rs`）。crate 缺失面以自寫協議層補齊，
逐項記於 §3 與 D 表。

## 1. gateway trait 擴充面（`mm-gateway::DeviceGateway`）

新增方法（全數有預設實作＝回 `unsupportedOperation`，既有測試替身不受影響；
兩後端覆寫 lockdown 路）：

| Swift（DeviceGatewayAPI） | Rust | 回傳 |
|---|---|---|
| `isDDIMounted()` | `is_ddi_mounted`（M4 已宣告，本段落實作） | bool |
| `mountDeveloperImage(image, signature)` | `mount_developer_image(&[u8], &[u8])` | () |
| `mountPersonalizedDdi(image, trustcache, manifest)` | `mount_personalized_ddi(&[u8], &[u8], &[u8])` | () |
| `installProvisioningProfile(profile)` | `install_provisioning_profile(&[u8])` | () |
| `removeProvisioningProfile(id)` | `remove_provisioning_profile(&str)` | () |
| `dumpProfiles(docsPath)` | `dump_profiles(&str)` | String（語義逐後端，見 §3／§4） |
| `removeApp(bundleId)` | `remove_app(&str)` | () |
| `sendIpaAfc(bundleId, ipaBytes)` | `send_ipa_afc(&str, &[u8])` | () |
| `sendAppBundleAfc(bundleId, appURL)` | `send_app_bundle_afc(&str, &str)`（路徑字串代 URL） | () |
| `installIpa(bundleId)` | `install_ipa(&str)` | () |
| `installAppBundle(bundleId, appName)` | `install_app_bundle(&str, &str)` | () |
| `wipeContainer(identifier)` | `wipe_container(&str)` | () |
| `debugApp(appId)` | `debug_app(&str)` | () |
| `debugProcess(pid)` | `debug_process(u32)` | () |
| `afcListDirectory(bundleId, path)` | `afc_list_directory(&str, &str)` | Vec\<String\> |
| `afcReadFile(bundleId, path)` | `afc_read_file(&str, &str)` | Vec\<u8\> |
| `afcGetFileInfo(bundleId, path)` | `afc_get_file_info(&str, &str)` | `AfcFileInfo{is_directory, file_size}` |

錯誤形狀不變：`MmErrorInfo`（DeviceGatewayError domain＋Swift reason 字串；
後端類別的 description 前綴差異——IdeviceGateway／LibimobiledeviceGateway
各自的 errorDescription 覆寫——已在 M1 錯誤總線以 domain 區分，本段沿用）。

## 2. RSP 工具（`mm-gateway::rsp` ← BaseDeviceGateway）

| Swift | Rust 對應 | 備註 |
|---|---|---|
| `RSPStopReply(rawPacket:)`：須以 `T` 開頭；signal＝第 2–3 字元 hex（不足或非法 → nil）；其餘以 `;` 切 pair、`:` maxSplits 1；key `thread`→threadId；key 全 hex 或 ∈{pc,sp,fp,lr,cpsr}→registers；餘→metadata | `RspStopReply::parse` | 逐字複刻（含「body 起點＝1＋實際取到的 hex 字元數」） |
| `RSPProcessInfo(rawResponse:)`：非空且不以 `E` 開頭；pid 先 hex 後十進位；name/executable 先 decodeHexASCII 失敗用原文；無 pid → nil；`matches(term)`＝name 或 executable 不分大小寫包含 | `RspProcessInfo::parse`／`matches` | key/val 兩側先 trim 空白（Swift 原碼有 trim，逐字複刻） |
| `stringToHex`：UTF-8 位元組 `%02x` 小寫 | `string_to_hex` | vAttachName 的 hex 是小寫（與 debugserver command 的 argv 大寫 hex 不同，見 §3 D-M5-5） |
| `decodeHexASCII`：偶數長、逐對 hex→位元組→UTF-8（失敗退 ASCII 解讀——Rust 以 from_utf8 失敗即 None 近似；純 ASCII 輸入兩者一致） | `decode_hex_ascii` | 非 hex 字元 → None |
| `findProcessPID` 詞彙生成：executableName（非空）→ bundlePath 去副檔名末段（非空、不等於 "App"、未重複）→ appId 以 `.`／`-`／`_` 切段（跳過小寫 ∈{com,org,net,io,app,ios}；跳過「10 字元、全英數、全大寫」段；先加去頭尾數字版（非空、不同於原段、未重複）、再加原段）→ appId 去最後一段的點分前綴 → 完整 appId | `process_search_terms`（純函數，決策表測試） | 切段以 Swift `split` 語義（空段丟棄） |
| 逐詞送 `vAttachName;<hex>`：回應非 nil、非空、不以 `E` 開頭即成功 → 送 `D`（錯誤吞）→ 回 **0**；全敗回 nil；sendCommand 本身的錯誤以 `try?` 吞掉續試 | `find_process_pid`（async，命令通道以 closure 注入） | 回 0 是 Swift 的「已直接 attach」慣例，呼叫端以 `pid > 0` 區分 |

## 3. gw-idevice 操作面（引擎＝idevice crate 0.1.29）

連接路：所有操作經 lockdown 路（TcpProvider 至 endpoint:62078 →
LockdownClient → start_session → start_service → connect），即 Swift
`performWithEitherService` 的 lockdown 半部。RP 模式於各操作入口回
`unsupportedOperation`（操作名）。`verifyInitialized` 僅查旗標（同 M3）。

crate 有現成 client 者直接用：misagent（MisagentClient）、mobile image
mounter（ImageMounter）、instproxy 查詢（InstallationProxyClient::get_apps）。
crate 缺失面自寫（皆以 idevice 的公開型別／方法為底）：

- **instproxy install/uninstall（D-M5-2）**：0.1.29 的
  InstallationProxyClient 只有 get_apps。自寫 plist 協議：Install＝
  `{Command: Install, PackagePath}`（installIpa 無 ClientOptions——Swift
  傳 nil；installAppBundle 加 `ClientOptions: {PackageType: Developer}`），
  讀回應至 `Status == "Complete"`；回應含 `Error` 鍵即失敗，reason
  `Failed to install IPA, error: (<Error 字串>)`／
  `Failed to install App directory, error: (<Error 字串>)`。
  Uninstall＝`{Command: Uninstall, ApplicationIdentifier}`，同樣讀至
  Complete／Error，reason `Failed to uninstall app, error: (<msg>)`。
  （Swift 的 FFI 在同一協議上判定；其錯誤訊息取 Error 欄——若真機回應的
  FFI 訊息組版不同，屬真機對照項。）
- **AFC 檔案操作（D-M5-3）**：0.1.29 的 AfcClient 有 list/mkdir/info/
  remove，但無 file open/read/write，且其 packet counter 私有。自寫
  `AfcSession`：包 `Idevice`，以 `idevice::afc::packet` 的公開型別
  （AfcPacket／AfcPacketHeader／AfcOpcode／AfcError）＋自有 counter
  實作全部操作；Status 回應比照 AfcClient::read 解碼（header_payload
  前 8B LE u64 → AfcError，非 Success 即 IdeviceError::Afc）。
  線格式：header 40B 全 LE（magic 0x4141504C36414643、
  header_payload_len 含 40B 頭長、entire_len 為全長）；
  FileOpen 的 header_payload＝mode u64 LE＋path＋NUL；
  Write 的 header_payload＝handle u64 LE、payload＝資料；
  Read 回 Data opcode 的 payload。同一連線只用 AfcSession 一套計數器。
- **house arrest（D-M5-4）**：0.1.29 無此 client。自寫：start_service
  `com.apple.mobile.house_arrest` → 連線送 `{Command: VendContainer,
  Identifier}` plist → 回應含 `Error` 鍵 → serviceError
  `Failed to vend container for <id>, error: (<Error>)`（FFI 同形判定）；
  成功後同一連線轉 AfcSession。
- **debug proxy（D-M5-5）**：`Idevice` 不暴露內部 socket（無
  into-stream），且 0.1.29 的 DebugProxyClient::read_response 不消費
  `#` 後的 2 字元 checksum（逐行核實），多命令序列會錯位。自寫
  `RspClient` 包 `Idevice`（send_raw/read_raw）：命令格式與 crate 相同
  （name＋argv 大寫 hex 串接、`$…#cc`、checksum 為位元組和 mod 256
  小寫 2 hex、ack 模式），回應讀取照 RSP 協議消費 checksum 並回 `+`。
  set_argv 照 crate 的 A 封包組版（`A<hex長>,<序>,<大寫hex>` 連接、
  首字元 'A'）但以 `$…#cc` 成幀送出（libimobiledevice 同形）。

逐操作對照（reason 皆 Swift 原文，`<msg>`＝IdeviceError 的 Display）：

| 操作 | 行為 | 失敗 reason |
|---|---|---|
| install_provisioning_profile | MisagentClient.install（Status==1 判定在 crate 內） | `Failed to install profile, error: (<msg>)` |
| remove_provisioning_profile | MisagentClient.remove | `Failed to remove profile, error: (<msg>)` |
| dump_profiles | copy_all → 逐筆切 `<?xml version=`…`</plist>`；有 UUID 寫 `<UUID>.mobileprovision`（全量原始位元組）＋`<UUID>.plist`（切出段）；無 UUID／解析失敗寫 `unknown_<i>.mobileprovision`（僅全量）；無 plist 段者跳過。dumpDir＝docsPath 去 `file://`，尾非 `/Profiles`／`/Profiles/` 才加 `/Profiles`；先建目錄（錯誤吞）；寫檔錯誤吞（`try?`）；回 dumpDir | copy 失敗：`Failed to copy profiles from misagent, error: (<msg>)` |
| remove_app | 自寫 Uninstall | 見 D-M5-2 |
| send_ipa_afc | **plain AFC**：mkdir `PublicStaging`、`PublicStaging/<id>`（錯誤忽略）；開 `PublicStaging/<id>/app.ipa` mode 4（Wr）；**單次 Write 全量**；關檔（defer 語義） | open：`Failed to open remote AFC file, error: (<msg>)`；write：`Failed to write to AFC file, error: (<msg>)` |
| install_ipa | 自寫 Install，path 同上 | 見 D-M5-2 |
| send_app_bundle_afc | mkdir staging／bundle／remoteAppPath（忽略錯誤）；遞迴走訪本地目錄：目錄→mkdir（忽略錯誤）、檔案→開 mode 4、非空才單次寫全量 | open：`Failed to open remote AFC file <path>: <msg>`；write：`Failed to write to AFC file <path>: <msg>`；列舉失敗：`Failed to enumerate <path>`（D-M5-13 走訪順序） |
| install_app_bundle | 自寫 Install＋PackageType=Developer，path `PublicStaging/<id>/<appName>` | 見 D-M5-2 |
| afc 三查詢 | 經 **house arrest** AfcSession：list／read（開 mode 1 RdOnly＋整檔讀）／info（st_ifmt **包含** `S_IFDIR` 判目錄、size 直取） | vend：見上；list：`Failed to list directory: <path>, error: (<msg>)`；read open：`Failed to open file: <path>, error: (<msg>)`；read：`Failed to read file: <path>, error: (<msg>)`；info：`Failed to get info for path: <path>`（無 msg 後綴，Swift 原樣） |
| wipe_container | house arrest 後 list `/`，逐項遞迴刪：目錄先遞迴子項（跳過 `.`／`..`）、再 remove_path；**remove 的錯誤只記錄不抛**，info／list 的錯誤照抛 | 各 AFC helper 的 reason |
| is_ddi_mounted | ImageMounter.copy_devices：任一 entry 滿足 MountPath=="/System/Developer" 且 PersonalizedImageType=="DeveloperDiskImage" 且（DiskImageType 缺或 =="Personalized"）→ true；**copy_devices 錯誤 → 回 false 不抛** | — |
| mount_developer_image | upload_image("Developer", image, signature) → mount_image("Developer", signature, 無 trustcache) | `Failed to upload developer image, error: (<msg>)`／`Failed to mount developer image, error: (<msg>)` |
| mount_personalized_ddi（lockdown 直連路） | endpoint 缺 → deviceEndpointIpNotAvailable（無 reason）；pairingFileData 缺 → connectionFailed `pairingFileData is nil`；TcpProvider → LockdownClient 先**無 session** 取 UniqueChipID，失敗才 start_session 後再取；值 plist 含 `Error` 鍵 → serviceError `Failed to get UniqueChipID: Prohibited`；session 失敗 → noConnection；再 ImageMounter.connect ＋ mount_personalized_with_callback（裝置有 manifest 先 query、失敗重連走 TSS——crate 內建，與 FFI 同形） | `Failed to get UniqueChipID`／`Failed to mount personalized DDI, error: (<msg>)`／`Failed to connect to image mounter` 等 Swift 原文 |
| debug_app（lockdown） | **一律 launchAppPre17（不看版本）**：getAppPaths（instproxy Lookup 單一 bundleId；查無 → `App not found: <id>`；Container／Path 空 → `Failed to resolve app paths`；查詢失敗 → `Failed to lookup app paths, error: (<msg>)`）→ startDebugserverService（lockdownd start_service `com.apple.debugserver` 得 port）→ connectDebugProxy 經 **usbmuxd**（env 位址 → get_devices 取第一台 → connect_to_device(port, label "minimuxer-debug")）→ RspClient 依序 QSetMaxPacketSize ["1024"]、QSetWorkingDir [container]、set_argv [bundlePath, bundlePath]、qLaunchSuccess、D | usbmuxd 路各步 connectionFailed reason 逐字複刻 Swift（Failed to get usbmuxd default addr／…was nil／Failed to create usbmuxd connection／…was nil／Failed to list usbmuxd devices／No devices found on usbmuxd／Failed to connect to debugserver port \<port\>／Debug device handle was nil）；命令：`Failed to send command to debug proxy: <name>, error: (<msg>)`；argv：`Failed to set debug proxy argv, error: (<msg>)`；start service：`Failed to start debugserver service, error: (<msg>)` |
| debug_app（RP）／debug_process（RP） | M7 | unsupported |
| debug_process（lockdown） | **Swift 原生即 unsupportedOperation**（D-M5-1） | reason 逐字：`Attaching to process by PID is only supported in .rppairing mode` |

## 4. gw-limd 操作面（引擎＝rusty_libimobiledevice，feature `limd`）

執行形狀：每操作 `spawn_blocking` 內走 Swift 的
`withDevice`（get_device by UDID）→ `withLockdown`（handshake client）→
`withService` 鏈。`verifyInitialized` 需旗標＋cachedUDID（同 M3）。
服務名照 Swift `DeviceService` 表：misagent＝
`com.apple.mobile.MCInstall.shim.remote`、mobileImageMounter＝
`com.apple.mobile.mobile_image_mounter`、installationProxy＝
`com.apple.mobile.installation_proxy`、houseArrest＝
`com.apple.mobile.house_arrest`、afc＝`com.apple.afc`、
debugserver＝`com.apple.debugserver`。

**描述子消費（D-M5-14）**：wrapper 形狀不一——misagent／house_arrest／
mobile_image_mounter 有 descriptor 型建構子（`Client::new(device,
descriptor)`），本段照 Swift withService 精確複刻：先以
`LockdowndClient::start_service(<DeviceService 名>)` 取 descriptor 再建
client（M3 註記的 start_service port 寫死 0 問題不經此面——descriptor
直接交給 typed client，不讀其 port）。instproxy／afc／debugserver 的
wrapper 只有 start_service 型建構子（C 庫以內建同名服務啟動，與
DeviceService 表同名），以之替代；服務啟動失敗的 reason 因此是各
client 建構子的錯誤碼而非 Swift 的 lockdownd_start_service 码，記為
已知形差（錯誤 domain／code 不變）。

錯誤 reason 的 `<n>`：wrapper 錯誤 enum 的反向碼表（比照 M3 慣例新建：
AfcError 為**正數**表 0..33、InstProxyError −1..−64／−100、
MisagentError −1..−4、MobileImageMounterError −1..−5／−100..−102、
HouseArrestError −1..−4、DebugServerError **正數** 1..5；
UnknownError／ForceSignedType 等不可逆者 → −999）。

| 操作 | 行為 | 失敗 reason |
|---|---|---|
| install_provisioning_profile | MisagentClient.install(Plist::new_data(bytes))；空緩衝 → `Invalid profile buffer` | `misagent_install failed with code <n>` |
| remove_provisioning_profile | MisagentClient.remove(id) | `misagent_remove failed with code <n>` |
| dump_profiles（lockdown） | 先照 Swift 建 dumpDir（副作用保留）；misagent copy(false) → **回該 plist 的 XML 字串**（非路徑——Swift 原行為，逐字複刻；facade .raw 直接透傳、.zip 會因路徑不存在失敗，同 Swift） | `misagent_copy_all failed with code <n>` |
| remove_app | InstProxyClient.uninstall(id, None) | `instproxy_uninstall failed with code <n>` |
| install_ipa | InstProxyClient.install(`PublicStaging/<id>.ipa`, None)——wrapper 以 status callback 等待終局（Complete→Ok、Error→(code, description)）；Swift 傳 nil callback（僅看啟動碼）。成功語義一致；失敗面更嚴（裝置回 Error 時 Swift 的 nil-callback 形狀不報、wrapper 報）——顯名分歧，reason：`instproxy_install failed with code <n>`（description 非空時附 `: <description>`） | 同左 |
| install_app_bundle | install(`PublicStaging/<id>/<appName>`, options{PackageType: Developer}) | 同上 |
| send_ipa_afc | plain afc：remotePath＝`PublicStaging/<id>.ipa`、**不 mkdir**、file_open **ReadWrite（AFC_FOPEN_RW）**、file_write 單次全量；空緩衝 → `Invalid IPA buffer` | `afc_file_open failed with code <n>`／`afc_file_write failed with code <n>` |
| send_app_bundle_afc | mkdir 三層（錯誤忽略 `_ =`）；遞迴走訪：目錄→mkdir（忽略）、檔案→open RW、非空才單次寫 | open：`afc_file_open failed for <path> with code <n>`；write：`afc_file_write failed for <path> with code <n>`；列舉：`Failed to enumerate <path>` |
| wipe_container | HouseArrestClient.send_command("VendContainer", id) **僅止於此、不刪檔**（Swift 原行為）；wrapper 的 send_command 合併 send＋get_result 兩步（D-M5-8：Swift 的兩條 reason 合成為第一條） | `house_arrest_send_command failed with code <n>` |
| afc 三查詢 | **忽略 bundleId**、plain afc：read_directory；read 開 RdOnly 後以 65536 chunk 迴圈讀至 bytesRead==0；info 解析鍵值對，st_ifmt **恰等於** `S_IFDIR`（與 gw-idevice 的「包含」不同，逐後端複刻）、st_size 解析失敗取 0 | `afc_read_directory failed with code <n>`／`afc_file_open failed with code <n>`／`afc_file_read failed with code <n>`／`afc_get_file_info failed with code <n>` |
| is_ddi_mounted | mounter lookup_image("Developer")：C 錯誤或無結果 → false；結果 dict 含 `ImageSignature` 鍵 → true | — |
| mount_developer_image | **D-M5-6**：Swift 不上傳、直接 mount_image（裝置路徑 `/tmp/DeveloperDiskImage.dmg`、簽名走記憶體）；wrapper 的 mount_image 只收本地檔案路徑（且把 canonical 化後的本機路徑當裝置路徑傳）。比照舊 crate 實績：image／signature 寫本地暫存檔 → upload_image → mount_image。已知損失：多一次上傳、裝置路徑不同 | upload：`mobile_image_mounter_upload_image failed with code <n>`（Swift 無此步、無原文可對）；mount：`mobile_image_mounter_mount_image failed with code <n>` |
| mount_personalized_ddi | **D-M5-7**：wrapper 無 mount_image_with_options、bindings 私有。改經 idevice crate over **UsbmuxdProvider**（mm-core 代理位址；pair record 由 mux 供應）執行 lockdown 直連路（UniqueChipID＋mount_personalized_with_callback），線形狀同 gw-idevice；reason 用 idevice 路字串（`Failed to mount personalized DDI, error: (<msg>)`），非 Swift limd 的 `mobile_image_mounter_mount_image_with_options failed with code <n>` | 見左 |
| debug_app（lockdown） | DebugServer（start_service 型）＋find_process_pid（僅 appId——無 bundlePath／executableName）；找不到 → serviceError（右）；pid>0 才送 `vAttach;<小寫hex>`、`D`；pid==0 表示 find 內已 attach 成功 | `App '<appId>' is not running. Please open the app and keep it in the background, then enable JIT.` |
| debug_process（lockdown） | 直接 `vAttach;<hex>`＋`D` | 命令失敗：`debugserver_command_new failed with code <n>`／`debugserver_client_send_command failed with code <n>`（D-M5-9） |

**D-M5-9（debugserver args 限制）**：wrapper 的
`DebugServerCommand::new` 對非空 args 的編碼損壞（逐行核實：每 arg 只取
首位元組且指標組版錯誤）。Swift limd 的實際呼叫集所有命令皆空 args
（payload 在 name 內：`vAttachName;<hex>`、`vAttach;<hex>`、`D`），本段
嚴格只以空 args 呼叫；wrapper send_command 無回應時回 `""`，對映
Swift 的 nil（find_process_pid 以「空 → 未命中」處理，一致）。

## 5. mm-core：Mounter 本體＋facade forwards（D-M4-4 結案）

`mm-core::Mounter`（← Mounter.swift 逐行）：相依以 seam 注入——
gateway（core 的 provider 現行 gateway）、endpoint IP（NetStack
endpoint）、下載器 `ImageDownloader`（生產＝reqwest；Swift 用
`Data(contentsOf:)` 同步 GET，失敗即 nil → downloadImage）。
proxyServer 在 Swift Mounter 僅持有未用，不複刻。

`mount(docsPath) -> Result<bool, MmErrorInfo>`：
1. 路徑正規化：去 `file://` 前綴（7 字元）、去**一個**尾斜線、加 `/DMG`；
   建目錄（錯誤忽略）。
2. endpoint.ip() 缺 → MinimuxerError.noDevice("Reachable device IP not found")。
3. gateway.is_ddi_mounted()（錯誤照抛）→ true 回 **false**（未新掛）。
4. pairing 為 lockdown 時 get_lockdown_value("ProductVersion")：取 `.`
   切分首段解析 major，失敗 → MinimuxerError.invalidProductVersion(v)；
   非 lockdown 預設 major＝17。
5. major<17（必為 lockdown）：pre-17 載入 → gateway.mount_developer_image；
   否則 personalized 載入 → gateway.mount_personalized_ddi。成功回 **true**。

pre-17 載入：`<DMG>/<ver>.dmg` 與 `.dmg.signature` 任一缺 → 下載
PRE17_VERSIONS_URL 的 JSON dict 查表（任一步失敗 →
downloadImage("Failed to retrieve pre-17 versions plist or find iOS <ver> DMG URL")）
→ 下載 zip 寫 `dmg.zip`、解壓至 `tmp/`、逐一檢查 tmp 子目錄（目錄且不含
`__MACOSX`）：內含 DeveloperDiskImage.dmg 者，先刪目標再搬移 dmg 與
signature 為 `<ver>.dmg`／`.dmg.signature`；zip 與 tmp 以 defer 語義刪除
（成功失敗皆刪）。最後讀兩檔，失敗 →
mount(.lockdown, "Unable to read pre-17 image files at: <dmg 路徑>")。
zip 下載／解壓／搬移的中段錯誤在 Swift 為原生 Cocoa 錯誤（非
MinimuxerError）→ facade catch-all .mount；Rust 以 domain `MounterIO`
的 MmErrorInfo 承接（D-M5-10：經 core 映射後形狀同 Swift——.mount＋
描述文字；文字內容為 Rust io 錯誤，與 Cocoa 文字不同，平台差異）。
解壓以 `zip` crate 代 ZIPFoundation。

personalized 載入（getOrDownload×3）：本地有 → 讀；無 → 下載（失敗 →
downloadImage("Failed to download file from <url>")）、寫本地、回資料。
三件：Image.dmg／Image.dmg.trustcache／BuildManifest.plist，URL 為
mm-common 既有 DDI_*_URL 常數（URL 無效分支
downloadImage("Invalid post-17 DDI URLs configured") 保留對映，常數恆有效）。

依賴新增：mm-core 加 `zip`、`reqwest`（rustls）、`serde_json`、`chrono`
（皆已在 workspace lockfile 內）。

facade（MinimuxerCore）：
- `mount_ddi`：任務管理照 Swift——記錄 last_docs_path、取消舊任務並
  await 其結果（錯誤忽略）、建新任務存入 state、await 之。Rust 形狀：
  spawn 工作 task（結果經 oneshot 送回）＋JoinHandle 存 state（stop／
  下一輪 mount 以 abort 取消）；被取消時 oneshot 斷線 → 回內部錯誤
  info（經 .mount 映射）。Mounter 以 core 現行 gateway＋endpoint 組裝。
- forwards 與 catchAll（逐字對映 MinimuxerImpl）：send_ipa_afc／
  send_app_bundle_afc → rwAfc；install_ipa／install_app_bundle →
  installApp；remove_app／wipe_container → uninstallApp；
  install_provisioning_profile → profileInstall；
  remove_provisioning_profile → profileRemove；afc_list_directory／
  afc_get_file_info → createAfc；afc_read_file → rwAfc；
  is_ddi_mounted／mount_ddi → mount{active}。
- `debug_app`／`attach_debugger` → createDebug，先 `ensure_ddi_mounted`：
  gateway.is_ddi_mounted() 錯誤吞為 false；未掛且 last_docs_path 缺 →
  MinimuxerError.mount(active, "DDI mount path not set")；否則
  mount_ddi(last_docs_path)（其 MinimuxerError 直接透傳）。
- `dump_profiles(docs_path, mode)` → createMisagent：`.raw` 直回
  gateway 結果；`.zip` 先 stage 到暫存目錄（temp_dir＋UUID 名）、
  gateway.dump_profiles(暫存路徑)、把回傳路徑以 zip crate 壓成
  `<docsPath>/Profiles-<ISO8601>.zip`（shouldKeepParent=false＝內容物
  置根層）、回 zip 路徑、defer 刪暫存。D-M5-11：檔名時間戳以 UTC
  `%Y-%m-%dT%H:%M:%SZ` 生成（Swift ISO8601DateFormatter 預設組版
  隨裝置時區可能不同——僅檔名層級）；zip 容器位元組與 ZIPFoundation
  不同，內容物集合相同。

## 6. RSD AFC 編解碼（`mm-gateway::rsd_afc`，本段落地、M7 接線）

照 limd Swift 的 `rsdAfc*` 逐行：magic 0x4141504C36414643、header 40B
全 LE（entire_len＝40＋header_payload＋payload；header_payload_len＝
40＋header_payload）、packetNum 由呼叫端持有（Swift 各操作開頭重置
為 0）。操作層（over 任意 AsyncRead＋AsyncWrite 串流）：
make_dir（path＋NUL，回應 `try?` 吞）、file_open（mode u64＋path＋NUL；
回 FileOpenRes 取 handle，handle 0 或回應不足 → 錯誤）、
file_close（回應吞）、write（header_payload＝handle；payload 以
kAfcChunkSize＝1MB 分塊逐塊送，每塊查 Status 回應：opcode 1 且
status≠0 → 錯誤）、send_packet／recv_response 原語。opcode 常數：
Status 1、MakeDir 9、FileOpen 13、Write 16、FileClose 20。
錯誤 reason 比照 Swift RSD 路字串（`AFC Write failed with status <n>`
等）。DESIGN M5 驗收的「RSD AFC 封包編解碼」以 fixtures＋記憶體
串流測試覆蓋；兩後端 RP 路徑的實際接線（RSD 服務流取得）屬 M7。

## 7. FFI 面（mm-ffi）

比照 M4 慣例只開 **core 面**（App 經 facade 使用；gateway 直呼面
不開操作面）：`mm_core_send_ipa_afc`、`mm_core_send_app_bundle_afc`、
`mm_core_install_ipa`、`mm_core_install_app_bundle`、`mm_core_remove_app`、
`mm_core_wipe_container`、`mm_core_debug_app`、`mm_core_attach_debugger`、
`mm_core_install_profile`、`mm_core_remove_profile`、
`mm_core_dump_profiles`（mode：MmProfileDumpMode Zip／Raw）、
`mm_core_afc_list_directory`、`mm_core_afc_read_file`、
`mm_core_afc_get_file_info`（新 shared struct MmAfcFileInfo）。
回傳形狀沿用 MmOutcome／MmStringOutcome／MmReadyOutcome 家族；
list 回 `Vec<String>`、read 回 `Vec<u8>`。

**D-M5-12（無安裝進度事件）**：Swift 的 gateway API 無進度回呼——
instproxy 的 PercentComplete 在兩後端的 FFI／C 呼叫內部被消費丟棄，
App 只見完成／失敗。故 M5 不開 EventSink 進度事件；若日後 App 需要
進度，屬新規格而非移植。

## 8. 驗收對照（DESIGN §4 M5）

cargo test（host）：
- RSD AFC 封包編解碼 fixtures＋記憶體串流 send_ipa 序列（§6）。
- RSP 回覆解析 fixtures：StopReply（T 包）、ProcessInfo（hex 名稱／
  pid 雙進位）、string_to_hex／decode_hex_ascii 邊界、find_process_pid
  詞彙決策表。
- Mounter 分流決策表：已掛載→false；endpoint 缺→noDevice；
  lockdown＋版本解析失敗→invalidProductVersion；pre-17 本地檔直用／
  缺檔走下載解壓；post-17 本地／下載三件；（lockdown,16)→developer、
  (lockdown,17)→personalized、(rppairing)→personalized 且不取版本。
- mock 線格式整合（延續 mock lockdownd）：instproxy 進度序列→
  Complete／Error 分支、misagent install/remove/copy_all、
  mounter copy_devices/lookup/upload/mount、AFC 記憶體 FS
  （send/read/list/info/wipe）、house arrest vend→AFC、debugserver
  RSP（launchAppPre17 全序列經 mock usbmuxd 轉接）、UniqueChipID
  無 session→補 session 分支。
- 錯誤映射：gateway MmErrorInfo → core MinimuxerError 的 catchAll
  對照（§5 表）逐項有測試。

真機（本段不執行，列回報待驗）：完整刷新／安裝一輪、移除 App、
dump profiles .zip 可解且內容與 Swift 版一致、JIT debugApp attach、
DDI 掛載（pre-17 與 personalized 兩路）、兩後端安裝同值對照。

## D 表（M5 顯名分歧與已知損失）

- **D-M5-1**：debug_process 在 lockdown 模式回 unsupportedOperation
  是 **Swift 原生行為**（reason 逐字），非待補實作；RP 路實作屬 M7。
- **D-M5-2**：gw-idevice 的 instproxy install/uninstall 為自寫 plist
  協議（idevice 0.1.29 無此 API）；完成／失敗判定與 reason 組版見 §3，
  FFI 訊息細節待真機對照。
- **D-M5-3**：gw-idevice AFC 以自寫 AfcSession（自有 counter）取代
  AfcClient 的零散方法，理由見 §3。
- **D-M5-4**：gw-idevice house arrest 自寫 VendContainer；回應 Error
  鍵的判定與 FFI 同形。
- **D-M5-5**：gw-idevice debug proxy 以自寫 RspClient 取代 crate
  DebugProxyClient（socket 不可得＋checksum 消費問題），命令格式
  與 crate 對齊、checksum 照協議消費。
- **D-M5-6**：gw-limd mount_developer_image 經暫存檔＋upload＋mount
  （Swift 無 upload、裝置路徑為 /tmp/DeveloperDiskImage.dmg、
  簽名走記憶體）——wrapper 形狀所限。
- **D-M5-7**：gw-limd mount_personalized_ddi 經 idevice 引擎 over
  usbmuxd 代理執行（wrapper 無 mount_image_with_options）；
  reason 字串隨 idevice 路。
- **D-M5-8**：gw-limd wipe_container 的 send／get_result 兩步 reason
  合併為第一條（wrapper 合併 API）。
- **D-M5-9**：gw-limd debugserver 命令僅限空 args（wrapper 編碼
  損壞）；Swift 實際呼叫集本即全空 args，無功能損失。
- **D-M5-10**：Mounter 中段 fs／下載錯誤以 domain `MounterIO` 承接，
  經 core 映射後與 Swift catch-all 同形；錯誤文字為 Rust io 文字。
- **D-M5-11**：dump .zip 檔名時間戳組版（UTC 'Z'）與 zip 容器位元組
  與 Swift／ZIPFoundation 不同；內容物集合相同。
- **D-M5-12**：無安裝進度事件（Swift 面不存在進度回呼）。
- **D-M5-13**：send_app_bundle_afc 走訪順序：Rust 以排序走訪，
  Swift 的 FileManager 列舉順序未定義；逐檔操作獨立、結果等價。
- **D-M5-14**：gw-limd withService 描述子消費僅三服務可精確複刻
  （misagent／house_arrest／mobile_image_mounter）；instproxy／afc／
  debugserver 以 wrapper start_service 型建構子替代（服務名同、
  啟動失敗 reason 的碼源不同）。
- **D-M5-15**：gw-idevice send_ipa_afc 的 IPA 以單次 AFC Write
  全量送出（與 FFI afc_file_write 單次呼叫同形）；裝置對單封包
  大小的實際上限待真機觀察（limd RSD 路的 1MB 分塊在 M7 隨
  rsd_afc 接線生效）。
