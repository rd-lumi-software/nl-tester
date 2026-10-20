# Hướng dẫn test độc lập HC V3

Tài liệu tạm này mô tả cách dùng Lumi Tester để test trực tiếp một Home
Controller V3, không cần chạy Android/iOS App.

## 1. Trạng thái hiện tại

Luồng hiện tại:

```text
YAML platform: lumi_hc
        |
        v
Profile implementation/protocolPack
        |
        v
V3 HC backend
        |
        +--> HTTP API của HC
        +--> WebSocket nếu flow dùng realtime
        +--> report/evidence
```

- Public platform của HC là `lumi_hc`.
- Backend hiện đã triển khai là `lumi_iot_v3_home_controller`.
- HCG1 chưa được triển khai; sau này HCG1 sẽ dùng backend/profile riêng nhưng
  vẫn có thể dùng `platform: lumi_hc`.
- Test điều khiển API không cần SSH, UART hoặc Android/iOS.
- SSH/UART chỉ cần khi bổ sung luồng diagnostic, recovery hoặc phần cứng ngoài
  phạm vi API HC.

## 2. Cấu trúc thư mục đề xuất

```text
lumi-tester/
├── tests/
│   └── hc/
│       └── v3/
│           ├── profiles/
│           │   └── lumi-iot-v3.yaml
│           └── switch-control.yaml
└── docs/
    └── hc-v3-standalone-testing.md
```

`profile` luôn được resolve tương đối với file flow YAML.

## 3. Chuẩn bị kết nối tới HC

Giả sử:

- HC chạy trên thiết bị SSD202D.
- Lumi Tester chạy trên máy tính.
- Địa chỉ HC là `192.168.1.50`.
- HTTP service chạy port `8080`.

IP trong profile là IP của HC, không phải IP máy chạy Lumi Tester.

```powershell
$env:LUMI_HC_URL = "http://192.168.1.50:8080"
$env:LUMI_HC_WS_URL = "ws://192.168.1.50:8080/ws"
```

Kiểm tra máy tính có truy cập được HC:

```powershell
Test-NetConnection 192.168.1.50 -Port 8080
Invoke-RestMethod "$env:LUMI_HC_URL/api/health_check"
```

Nếu HC dùng HTTPS/WebSocket bảo mật thì dùng `https://` và `wss://` tương ứng.

## 4. Profile V3

Tạo `e2e/hc/v3/profiles/lumi-iot-v3.yaml`:

```yaml
implementation: lumi_iot_v3_home_controller
protocolPack: lumi_iot_v3_home_controller
protocolVersion: "0.2.0"
packVersion: "0.1.0"

target:
  chip: ssd202d
  firmware: "<firmware-version>"

endpoint:
  baseUrl: "${LUMI_HC_URL}"
  websocketUrl: "${LUMI_HC_WS_URL}"

auth:
  mode: none

capabilities:
  - health
  - device_discovery
  - device_state
  - device_control

evidence:
  root: output/hc-v3
  maxRawBytes: 4096
  maxWebSocketFrames: 32
  redactFields:
    - dev_key
    - nwk_key
    - app_key
    - authorization
    - cookie
    - token

retry:
  safeHttpMaxAttempts: 3
  controlMaxAttempts: 0
  websocketReconnectMaxAttempts: 1
```

`device_state` là capability readback qua HTTP. Nếu flow dùng các command
WebSocket như `wsSubscribe`, `wsExpect` hoặc `wsSnapshot`, bổ sung:

```yaml
  - realtime_cell_readback
```

Không đặt `/api` vào `baseUrl`; backend tự ghép các route API.

## 5. Flow bật/tắt thiết bị

Tạo `e2e/hc/v3/switch-control.yaml`:

```yaml
gateway:
  platform: lumi_hc
  profile: profiles/lumi-iot-v3.yaml

tags:
  - smoke
defaultTimeout: 15000
---
- hcHealth

- hcGetEntities

- hcGetState:
    entity: "9007199254740993"

- hcControl:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: true

- hcWaitState:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: true

- hcControl:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: false

- hcWaitState:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: false
```

Mỗi dòng ở trên là một action nguyên tử. Runner không tự chèn action khác khi
gặp `hcControl`. Nếu flow cần xác nhận độc lập qua WebSocket rồi đọc lại bằng
API, hãy ghép tường minh bằng `runFlow` hoặc một subflow:

```yaml
- runFlow:
    path: subflows/hc-control-confirmed.yaml
    target: gateway
```

Nội dung `subflows/hc-control-confirmed.yaml` có thể là:

```yaml
platform: lumi_hc
gateway:
  platform: lumi_hc
  profile: profiles/lumi-iot-v3.yaml
---
- wsSubscribe:
    entity: "9007199254740993"
- hcControl:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: true
- wsExpect:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: true
- hcWaitState:
    entity: "9007199254740993"
    state:
      - idx: 0
        value: true
```

Nếu profile không có WebSocket hoặc API readback riêng, chỉ ghép các action mà
profile khai báo capability; không có sequence HC bắt buộc được tự động thêm.

Thay các giá trị sau bằng dữ liệu thật của thiết bị:

- `entity`: device ID lấy từ `hcGetEntities` hoặc API HC.
- `idx`: state index tương ứng với công tắc trên thiết bị.
- `value`: trạng thái cần kiểm tra.

> Nên đặt entity ID trong dấu nháy. Các ID lớn hơn giới hạn số an toàn của
> JavaScript cần được giữ nguyên dạng chuỗi, ví dụ `"9007199254740993"`.

### Ý nghĩa từng command

| Command | Hành động V3 |
|---|---|
| `hcHealth` | `GET /api/health_check` |
| `hcGetEntities` | `GET /api/devices/` |
| `hcGetState` | `GET /api/devices/{id}/state` |
| `hcControl` | `POST /api/devices/{id}/control`, sau đó đọc state để readback |
| `wsSubscribe` | Mở subscription realtime; chỉ dùng khi profile có capability tương ứng |
| `wsExpect` | Xác nhận event/state realtime theo đúng action đã viết trong flow |
| `hcWaitState` | Đọc state có giới hạn để chờ trạng thái khớp |

Vì `hcControl` đã tự kiểm tra readback, `hcWaitState` chỉ cần khi thiết bị
thay đổi trạng thái bất đồng bộ hoặc muốn thể hiện rõ bước chờ xác nhận.
`wsExpect` và `hcWaitState` là các action độc lập, không phải companion action
được backend hoặc orchestrator tự chèn.

## 6. Validate, list và run

Chạy từ thư mục root repository:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- validate `
  .\lumi-tester\e2e\hc\v3\switch-control.yaml --json
```

Kết quả cần có `"valid": true`.

Xem command index trước khi chạy:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- list `
  .\lumi-tester\e2e\hc\v3\switch-control.yaml --json
```

Chạy test thật:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- run `
  .\lumi-tester\e2e\hc\v3\switch-control.yaml `
  --platform lumi_hc `
  --report `
  --snapshot `
  --events-jsonl `
  --output .\output\hc-v3-switch
```

`--platform lumi_hc` có thể bỏ nếu YAML đã khai báo platform, nhưng giữ lại
giúp lệnh dễ đọc và tránh chạy nhầm target.

Muốn lặp lại toàn bộ chu trình bật/tắt nhiều lần:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- run `
  .\lumi-tester\e2e\hc\v3\switch-control.yaml `
  --platform lumi_hc `
  --repeat 10 `
  --report `
  --events-jsonl `
  --output .\output\hc-v3-repeat
```

## 7. Report và evidence

Sau khi chạy, xem các file:

```text
output/hc-v3-switch/run.json
output/hc-v3-switch/test-results.json
output/hc-v3-switch/events.jsonl
```

Trong đó:

- `run.json`: trạng thái lifecycle, command index và kết quả thực thi.
- `test-results.json`: report tổng hợp để đọc bằng máy hoặc xem qua tooling.
- `events.jsonl`: event theo từng bước chạy.
- `output/hc-v3/`: raw HTTP/readback evidence theo `evidence.root` trong
  profile.

Một control pass hợp lệ phải có đủ action, readback, assertion và evidence.
HTTP response thành công nhưng state readback không khớp sẽ không được coi là
pass.

## 8. Debug khi lỗi

Không đếm command index bằng tay; dùng `list --json`. Sau đó chạy lại command
lỗi, ví dụ command index `3`:

```powershell
cargo run --manifest-path .\lumi-tester\Cargo.toml -- run `
  .\lumi-tester\e2e\hc\v3\switch-control.yaml `
  --platform lumi_hc `
  --command-index 3 `
  --report `
  --snapshot `
  --events-jsonl `
  --output .\output\hc-v3-debug
```

Các lỗi thường gặp:

| Lỗi | Kiểm tra |
|---|---|
| `connection refused` | HC service, IP, port và firewall |
| `real HC endpoint environment is unresolved` | Đã set `LUMI_HC_URL` và `LUMI_HC_WS_URL` chưa |
| `missing capability` | Profile có `device_control` và `device_state` hoặc `realtime_cell_readback` chưa |
| `readback mismatch` | `entity`, `idx`, kiểu giá trị và trạng thái vật lý của thiết bị |
| `profile not found` | Đường dẫn profile có tương đối với file flow không |

## 9. Khi thêm HCG1

Flow semantic vẫn có thể giữ nguyên:

```yaml
gateway:
  platform: lumi_hc
  profile: profiles/hcg1.yaml
```

Profile HCG1 sẽ đổi `implementation`, `protocolPack`, endpoint và capability.
Backend HCG1 sẽ chuyển `HcCommand` như `hcControl` và `hcGetState` sang protocol
riêng. Hiện tại chỉ profile V3 được đăng ký, nên chưa thể chạy HCG1 chỉ bằng
cách đổi tên profile.
