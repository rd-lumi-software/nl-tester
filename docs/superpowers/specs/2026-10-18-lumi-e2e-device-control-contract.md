# Contract Phase 1: Lumi E2E điều khiển một device đơn

Trạng thái: design baseline cho Planner review  
Ngày: 2026-10-18 (GMT+7)  
Phạm vi: lumi-tester và implementation tham chiếu duy nhất
D:\lumi-iot-v3

Tài liệu này là kết quả audit SRS v0.2 với source và tài liệu hiện tại. Nó chỉ
đóng contract semantic và các wire fact đã có nguồn xác nhận. Chi tiết nào còn
mâu thuẫn hoặc chưa được source xác nhận được đánh dấu needs clarification;
không dùng giá trị minh họa để biến gap thành contract.

## 1. Quyết định và ranh giới

Vertical slice này chạy platform: lumi_hc với journey: system_journey, tập
trung vào một Cell device duy nhất:

- health/readiness;
- discovery bằng danh sách device;
- đọc state của một device;
- điều khiển state của một device;
- WebSocket subscribe, realtime state/config/event readback và snapshot;
- canonical mapping cho Device/Cell, ID, slot, value, event, timestamp,
  source và correlation;
- result passed, failed, blocked, error và evidence raw/normalized có giới hạn,
  có redaction;
- unit, mock HTTP/WS contract, protocol-pack conformance và smoke test
  deterministic.

user_journey, App, automation/rule, scene, schedule, group control, Cloud,
MQTT, SSH, serial, hardware-jig, bridge scan, config/command tùy biến và mọi
capability mở rộng khác đều là deferred. Source có route hoặc type cho một
capability không làm capability đó trở thành acceptance gate của Phase 1.

Không có runner YAML thứ hai. YAML hiện có tiếp tục đi qua parser, command
index, runner state, event JSONL và report pipeline hiện tại. E2E được thêm như
một nhánh domain riêng bên trong command model và một E2E session riêng.

## 2. Fact matrix: SRS, product và v3

### 2.1 Lumi Tester hiện tại — đã xác nhận

| Fact | Nguồn | Tác động thiết kế |
| --- | --- | --- |
| TestFlow có platform, defaultTimeout, tags, commands; chưa có journey hoặc profile. | lumi-tester/src/parser/types.rs:9-35, parser/yaml.rs:234-339 | Bổ sung field header theo kiểu additive; không đổi hành vi header App cũ. |
| Platform hiện chỉ có Android/Android Auto/iOS/Web/macOS/Windows. | lumi-tester/src/parser/types.rs:516-530 | Thêm platform semantic lumi_hc, nhưng không đưa nó vào App PlatformDriver. |
| Command được parse từ list YAML qua parse_command_value và parse_command_with_params; list lấy display_name() và index theo thứ tự. | parser/yaml.rs:381-523, src/main.rs:1778-1787 | E2E command phải có display name ổn định và giữ index 0-based. |
| TestCommand là enum rất lớn và TestExecutor match toàn bộ command, nhận một Box<dyn PlatformDriver>. | parser/types.rs:759-1046, runner/executor.rs:19-56, 1520-... | Không đưa HC state vào App driver; cần E2E dispatch/session branch ở boundary runner. |
| TestContext đã giữ base path, output, env/vars và default timeout; path tương đối được resolve theo flow. | runner/context.rs:7-105 | Profile path và evidence root resolve theo flow/profile, không theo checkout path cố định. |
| State hiện tại chỉ có Passed, Failed, Skipped, Retrying; flow chỉ có Passed, Failed, PartiallyPassed. | runner/state.rs:4-14, 25-119, 136-144 | E2E cần result envelope riêng để thêm blocked/error mà không đổi nghĩa report App cũ. |
| Event JSONL serialize TestEvent và hiện có session/flow/command passed/failed/retry/skip. | runner/events.rs:6-140 | Thêm event metadata E2E theo additive schema; giữ tên và shape cũ cho App flow. |
| Report chuẩn hiện suy ra pass/fail từ số failed, JUnit coi mọi failure là assertion failure. | report/types.rs:4-90, report/json.rs:24-145, report/junit.rs:9-115 | E2E status phải nằm trong extension structured, không để blocked bị coi là product pass/fail. |
| run_tests luôn chọn App driver theo platform trước khi executor chạy. | runner/mod.rs:19-... | Entry point phải nhận diện E2E trước bước khởi tạo App driver. |
| Product đã có reqwest, tokio-tungstenite, futures-util, serde, serde_json, Tokio; runtime HC nằm dưới src/hc/backends/v3/runtime. | lumi-tester/Cargo.toml, directory lumi-tester/src | Đây là boundary mới; không cần suy ra dependency hoặc API từ project HC khác. |

### 2.2 v3 HTTP — đã xác nhận hoặc gap

| Semantic | Wire fact | Trạng thái |
| --- | --- | --- |
| Health | GET /api/health_check; router source nest /api, health route /health_check; HTTP 200 JSON { "status": true }. | confirmed — bin/home-controller/src/transport/http/mod.rs:28-39, health.rs:7-12 |
| Device list | device_router nest /devices, collection handler là route "/", trả ApiResponse<Vec<DeviceEntity>>. HC triển khai thực tế nhận GET /api/devices (không có slash cuối). | confirmed by live HC route check; adapter và contract test dùng /api/devices. |
| State read | GET /api/devices/{device_id}/state; path được parse thành u64; 200 body là Vec<At0micDataValue>, không có wrapper và không có idx trong từng phần tử. | confirmed — device.rs:86, 133-141; services/devices.rs:514-524 |
| State control | POST /api/devices/{device_id}/control; body là JSON array [{ "idx": <usize>, "value": <raw JSON> }]; handler tạo request với x-request-id tùy chọn, nhưng response thành công chỉ { "status": true }. | confirmed — device.rs:143-162; services/devices.rs:34-70; Postman collection control examples |
| Control completion | HTTP handler gọi runtime dispatch và trả status true; không chờ physical/device ACK, không trả request ID và không có endpoint control response readback. | confirmed — services/devices.rs:448-464; correlation/readback policy vẫn phải là state/event observation. |
| Config/execute | POST .../config và POST .../execute tồn tại, với map config hoặc {command,payload}. | confirmed but deferred — không thuộc single-device state-control baseline. |
| Error | Domain errors được trả {code,msg} với status 400/409/500 tùy loại; lỗi parse/route do Axum có thể không đi qua HomeControllerError. | confirmed with boundary; adapter phải giữ status/body raw và không giả định mọi HTTP error là {code,msg} — common.rs:33-115. |
| Auth | Source hiện không có auth middleware cho HTTP router; control headers chỉ lấy request/user/app metadata. | confirmed: local auth.mode: none; không suy ra cho implementation khác. |

DeviceEntity list không phải payload nhỏ: source có thể trả network_data chứa
dev_key, nwk_key, app_key. Raw evidence bắt buộc redact các trường này trước
khi ghi artifact.

### 2.3 v3 WebSocket — đã xác nhận hoặc gap

| Fact | Trạng thái và nguồn |
| --- | --- |
| URL là ws(s)://{host}:{port}/ws?clientId={id}; clientId tùy chọn, server tự tạo UUID v7 nếu bỏ trống. | confirmed — docs/protocols/realtime-websocket.md:13-23, bin/home-controller/src/transport/http/ws.rs:80-89 |
| Client JSON subscribe: {"cmd":"subscribe","payload":["device_manager"]} hoặc "cell:<cell_id>"; response là {"type":"subscribe_response","payload":[...]}. Unsubscribe tương ứng. | confirmed — ws.rs:45-64, 166-195; docs realtime-websocket.md:25-41 |
| Server data envelope là {"type":"data","payload":<RealtimeOutgoingDto>}. Cell data có payload.channel = "cell_runtime_data", data.cell_id, data.content. | confirmed — ws.rs:55-64, 210-243; protocol/src/dto/realtime.rs:100-124 |
| Cell content gồm cell_state_changed với list {idx,value}, cell_config_changed với list {key,value}, hoặc cell_event với {event,params}. | confirmed — protocol/src/dto/realtime.rs:115-124, pubsub.rs:135-189 |
| device_manager có device_added/device_removed; đó là discovery/lifecycle event, không phải baseline control readback. | confirmed but deferred from smoke; docs realtime-websocket.md:43-99, source pubsub.rs:191-230 |
| RPC request dùng cmd: "rpc", payload {request_id,data}; data hỗ trợ CellSnapshot(cell_id). RPC response giữ cùng request_id, trả CellSnapshot(Vec<RealtimeCellState>) hoặc Error(String). | confirmed — ws.rs:33-43, 196-211, 249-319; realtime.rs:89-98 |
| CellSnapshot server giới hạn handler 200 ms; snapshot map state vector bằng enumerate() thành idx 0-based. | confirmed — ws.rs:255-267, 298-318; ý nghĩa tương ứng với declared spec.state[idx] vẫn là gap nếu spec không 0-based. |
| Không có replay sau reconnect; buffer subscriber 100 và message mới có thể bị drop; reconnect phải subscribe lại và gọi HTTP discovery/state để đồng bộ. | confirmed — docs realtime-websocket.md:144-155; source pubsub.rs:47-75 |
| Server parse lỗi incoming chỉ log và không trả error frame; server không cung cấp correlation cho event state. | confirmed — ws.rs:109-121; RealtimeOutgoingDto không có correlation field. |

### 2.4 Canonical mapping — gap cần giữ nguyên

Các type v3 xác nhận rằng At0micCellId là u64 không dấu, serialize thành chuỗi
decimal và chấp nhận string hoặc số khi deserialize
(protocol/src/models/cell.rs:54-75, 190-251). DeviceId serialize qua
At0micCellId (protocol/src/models/device.rs:215-230); HTTP path lại parse u64
(device.rs:135-140). Vì vậy adapter phải giữ decimal ID dưới dạng chuỗi ở
canonical/evidence và chỉ parse u64 tại boundary URL.

DeviceEntity xác nhận các field id, cell_model_id, cell_idx, hub_id, protocol,
network_state, spec, profile, snapshot, config
(protocol/src/entities/device.rs:62-74). cell_model_id source serialize thành
JSON number (models/device.rs:252-282); canonical layer không được ép qua
JavaScript Number, nên có thể giữ decimal string song song với raw value.

At0micCellSpec có input, state, output, commands, events, configs và At0micIdx
là usize (models/cell.rs:22-45, 674-736). Tuy nhiên:

1. HTTP state trả vector value không có slot index.
2. Repository v3 cập nhật vector bằng current_state.get_mut(idx.value())
   (packages/adapters/iot-kv-adapter/src/repositories/device.rs:495-531).
3. WebSocket snapshot tạo idx bằng enumerate() (ws.rs:312-318).
4. Runtime event giữ idx trong input/output (pubsub.rs:141-151).
5. Protocol core mô tả idx là index nhưng không chốt correlation SET/INVOKE
   với ACK (protocol-core-spec-0.2.0.md:96-110, 208-220). Một số Postman
   example dùng idx 1/2/3 trong khi source tests/spec examples dùng idx 0/1.

Do đó Phase 1 không được tự tạo alias power, không suy idx từ tên và không coi
vector position, declared slot và DeviceEntity.cell_idx là một field. Protocol
pack phải khai báo mapping cụ thể; nếu không có mapping thì semantic command chỉ
được dùng explicit wire index và report cả vector_index lẫn slot_idx nếu xác
định được.

## 3. Contract semantic Phase 1

### 3.1 Header

Header bắt buộc của một system flow:

    platform: lumi_hc
    journey: system_journey
    profile: profiles/lumi-iot-v3-home-controller.yaml
    defaultTimeout: 15000
    tags: [smoke, device-control]
    ---
    - hcHealth
    - hcGetEntities
    - hcGetState:
        entity: "<CELL_ID>"
    - wsSubscribe:
        channels: ["cell:<CELL_ID>"]
    - hcControl:
        entity: "<CELL_ID>"
        state:
          - idx: 0
            value: true
    - wsExpect:
        entity: "<CELL_ID>"
        event:
          type: cell_state_changed
          state:
            - idx: 0
              value: true
    - hcWaitState:
        entity: "<CELL_ID>"
        state:
          - idx: 0
            value: true

capability: power không nằm trong baseline. Alias chỉ được bật ở một profile
pack có mapping khai báo rõ tới input/state index; không có mapping thì parser
hoặc preflight phải báo unsupported_capability/blocked, không đoán.

### 3.2 Profile

Profile được resolve tương đối với thư mục flow, theo cùng nguyên tắc path của
TestContext; tuyệt đối không nhúng path checkout, IP lab cố định, serial port
hoặc credential vào semantic YAML/repository.

Schema semantic tối thiểu:

    implementation: lumi_iot_v3_home_controller
    protocolPack: lumi_iot_v3_home_controller
    protocolVersion: "0.2.0"
    packVersion: "0.1.0"
    target:
      chip: "<LUMI_TARGET_CHIP hoặc unknown>"
      firmware: "<LUMI_FIRMWARE hoặc unknown>"
    endpoint:
      baseUrl: "<LUMI_HC_BASE_URL>"
      websocketUrl: "<LUMI_HC_WS_URL>"
    auth:
      mode: none
    capabilities:
      - health
      - device_discovery
      - device_state
      - device_control
      - realtime_cell_readback
    evidence:
      root: ./output
      maxRawBytes: 65536
      maxWebSocketFrames: 200
      redactFields: [dev_key, nwk_key, app_key, authorization, cookie, token]
    retry:
      safeHttpMaxAttempts: 2
      controlMaxAttempts: 0
      websocketReconnectMaxAttempts: 2

endpoint có thể được resolve từ environment/profile secret reference ở runtime;
giá trị credential thật không được ghi vào profile mẫu hoặc evidence. control
không retry mặc định vì v3 không trả correlation/ACK cho HTTP dispatch; chỉ retry
khi profile sau này khai báo semantics idempotent và được case yêu cầu rõ.

Preflight chỉ kiểm tra capability mà case thật sự cần:

- thiếu profile, endpoint, pack hoặc capability bắt buộc: blocked;
- không kết nối được target ở preflight: blocked, không phải product fail;
- HTTP/WS lỗi sau preflight: error, trừ khi nhận được response/event hợp lệ
  chứng minh product assertion không đạt thì failed.

### 3.3 Semantic operations được phép

| Operation | Mục đích | Evidence bắt buộc |
| --- | --- | --- |
| hcHealth | Kiểm tra health/readiness | raw HTTP, normalized status |
| hcGetEntities | Lấy danh sách DeviceEntity và chọn đúng một entity | raw HTTP, normalized identity/spec; redact network data |
| hcGetState | Đọc vector state của entity | raw HTTP, normalized vector/slot mapping |
| hcControl | Gửi một hoặc nhiều {idx,value} explicit tới một entity | raw request/response, normalized dispatch result |
| hcWaitState | Poll HTTP state theo timeout bounded; có thể dùng sau event | raw readback và normalized match |
| wsSubscribe | Kết nối và subscribe channel khai báo | URL metadata đã redact, subscribe/response frames |
| wsExpect | Chờ và match cell state/config/event | bounded raw frames, normalized event và match result |
| wsSnapshot | Gọi RPC CellSnapshot để readback/fallback sau reconnect | request/response raw, normalized snapshot |

hcGetEntities phải chọn một device/Cell và fail nếu case vô tình điều khiển
nhiều entity. Group/scene/config/execute/bridge scan không được thêm vào
semantic baseline này.

## 4. v3 wire contract Phase 1

### 4.1 HTTP

#### Health

    GET /api/health_check
    200 application/json
    {"status": true}

status: false trong một preflight là môi trường chưa readiness (blocked); nếu
một assertion độc lập chủ động yêu cầu health true và nhận false thì là failed
với category product_assertion.

#### Discovery

Source handler trả Vec<DeviceEntity>, không phải {devices:[...]}:

    GET <collection route>
    200 application/json
    [
      {
        "id": "<decimal At0micCellId>",
        "cell_model_id": <u64 JSON number>,
        "cell_idx": <usize>,
        "hub_id": "<decimal u64>",
        "protocol": "virtual|ble|ip|zigbee|knx",
        "network_state": "Pending|Activated|Destroying|Removed",
        "network_data": null,
        "spec": {"input": [], "state": [], "output": []},
        "profile": {},
        "snapshot": {},
        "config": {}
      }
    ]

Route registration trong source là nest("/devices", ...route("/", ...)),
nhưng HC triển khai thực tế nhận path canonical /api/devices; request có slash
cuối trả 404. Adapter và contract test không tự coi hai path là alias.

#### State

    GET /api/devices/{device_id}/state
    200 application/json
    [false, 100.0, 4000.0]

device_id trong path là decimal u64 không có dấu quote. Body là vector raw
At0micDataValue; vị trí vector không tự mang idx.

#### Control

    POST /api/devices/{device_id}/control
    Content-Type: application/json
    [
      {"idx": 0, "value": true},
      {"idx": 1, "value": 80.0}
    ]

    200 application/json
    {"status": true}

x-request-id được source đọc nếu có và parse như u64 decimal, nhưng response
không echo ID; x-user-id/x-app-id chỉ tạo ClientIdentity và mặc định unknown.
HTTP 200 chỉ chứng minh dispatch được chấp nhận ở boundary runtime, không chứng
minh device vật lý đã áp dụng. Case phải chờ WebSocket event hoặc đọc state lại,
rồi match entity + index/value trong bounded time window.

#### Error

Với domain error đã đi qua HomeControllerError, shape là:

    {"code":"PROTOCOL_ERROR|RUNTIME_ERROR|REPOSITORY_ERROR|BRIDGE_ERROR|UNKNOWN_ERROR","msg":"..."}

Status phụ thuộc error (400, 409 cho scan conflict, hoặc 500). Adapter phải lưu
status và raw body; body không đúng {code,msg} được phân loại
invalid_payload/http_error thay vì deserialize mù.

### 4.2 WebSocket

Kết nối:

    GET ws(s)://<host>:<port>/ws?clientId=<unique-client-id>

Client phải dùng một clientId mới cho mỗi session. Không dựa vào server replay.

Subscribe request/response:

    {"cmd":"subscribe","payload":["cell:<cell_id>"]}
    {"type":"subscribe_response","payload":["cell:<cell_id>"]}

Data frame state:

    {
      "type":"data",
      "payload":{
        "channel":"cell_runtime_data",
        "data":{
          "cell_id":"<decimal At0micCellId>",
          "content":{
            "type":"cell_state_changed",
            "payload":[{"idx":0,"value":true}]
          }
        }
      }
    }

Các content hợp lệ khác là cell_config_changed với [{key,value} và cell_event
với {event,params}. Config/event không phải control acceptance nếu case không
khai báo observation tương ứng.

RPC request:

    {"cmd":"rpc","payload":{
      "request_id":"<decimal u64>",
      "data":{"cell_snapshot":"<decimal At0micCellId>"}
    }}

Response:

    {"type":"rpc","payload":{
      "request_id":"<same decimal string>",
      "data":{"cell_snapshot":[{"idx":0,"value":true}]}
    }}

Lỗi RPC là data.error theo external serde enum; handler snapshot có timeout nội
bộ 200 ms. request_id của RPC là correlation wire đã xác nhận. Event state không
có request ID; không được tự thêm correlation giả.

Khi reconnect:

1. đóng session cũ và tạo clientId mới;
2. reconnect tối đa theo profile;
3. resubscribe các channel đã khai báo;
4. dùng wsSnapshot hoặc hcGetState để đồng bộ current state;
5. báo error nếu không thể kết luận sau retry bound.

Vì server không replay event và có thể drop buffer khi đầy, không coi việc không
nhận event sau reconnect là bằng chứng device không đổi. Nếu snapshot/state
readback chứng minh giá trị cuối đúng nhưng event bị lỡ, result của action có thể
passed với evidence tier readback_without_event, còn assertion bắt buộc về event
phải failed nếu không có event trong cửa sổ quan sát.

## 5. Canonical model và mapping rule

Canonical model tối thiểu:

    EntityRef {
      entity_id: DecimalId,
      entity_kind: device_cell,
      cell_id: DecimalId,
      device_id: DecimalId,
      cell_model_id: DecimalId,
      cell_idx: u64,
      protocol: string,
    }

    SlotState {
      entity: EntityRef,
      slot_idx: u64?,
      vector_index: u64?,
      ref: declared_state_ref?,
      value: raw JSON value,
      source: http_state | websocket_event | websocket_snapshot,
      observed_at: RFC3339 runner timestamp,
      wire_timestamp: null unless a future pack declares one,
      correlation_id: optional wire RPC id only,
    }

    CellEvent {
      entity: EntityRef,
      event_type: string,
      params: raw JSON object,
      source: websocket_event,
      observed_at: RFC3339 runner timestamp,
      correlation_id: none for current v3,
    }

Rules:

- DecimalId là string decimal trong canonical JSON/evidence; parser phải reject
  negative, fractional, scientific hoặc vượt u64. Raw JSON vẫn được giữ nguyên.
- v3 DeviceEntity.id là DeviceId được serialize bằng At0micCellId. Pack ghi rõ
  device_id và cell_id có cùng wire value ở implementation này, nhưng canonical
  type vẫn tách hai khái niệm để không làm chúng thay thế cho nhau ở
  implementation khác.
- cell_model_id là model identity, không phải cell_id; cell_idx là index của
  Cell trong DeviceEntity, không phải state slot.
- spec.state[idx], spec.input[idx], spec.commands[name], spec.events[name] chỉ
  tạo canonical ref khi field tồn tại trong payload. Không tạo ref cho alias
  power nếu profile/pack không khai báo mapping.
- HTTP vector giữ vector_index theo vị trí response. Chỉ gắn slot_idx khi pack
  đã xác nhận quy tắc vector/index. WebSocket event giữ wire idx; snapshot giữ
  idx server gửi và không được âm thầm đổi sang declared spec index.
- State/config/event phải giữ raw value; normalized value chỉ đổi shape ở field
  đã được pack khai báo. Không ép mọi number thành integer hoặc mọi string ID
  thành JavaScript number.
- Timestamp của v3 realtime frame không có trong DTO. observed_at là thời điểm
  collector nhận frame; không gọi nó là device timestamp.

## 6. Result, evidence và phân loại lỗi

### 6.1 Result status

| Status | Khi dùng | Không được dùng khi |
| --- | --- | --- |
| passed | Action, required assertion, readback/evidence bắt buộc đều hoàn tất. | Chỉ có HTTP {status:true} nhưng chưa có readback nếu case yêu cầu readback. |
| failed | Nhận được product response/event/state hợp lệ nhưng assertion không đạt; ví dụ state không đổi, event sai value, HTTP domain response là lỗi cho positive operation. | Không parse được wire hoặc không biết target có nhận request hay chưa. |
| blocked | Preflight thiếu profile/endpoint/capability, pack conformance chưa pass, hoặc real lab unavailable trước runtime assertion. | Sau khi action đã bắt đầu và adapter mất khả năng kết luận. |
| error | Runner/adapter không thể kết luận: malformed payload/frame, transport error sau preflight, close không recover được, timeout không có đủ evidence, cancellation. | Response hợp lệ chứng minh product assertion fail. |

blocked là môi trường/capability status, không phải product pass/fail. Mock pass
phải ghi evidence_tier: mock_contract và không được nâng thành real-lab pass.

### 6.2 Evidence envelope

Mỗi E2E command ghi extension structured, không phá schema App cũ:

    {
      "e2e": {
        "status": "passed|failed|blocked|error",
        "category": "preflight|http_error|invalid_payload|timeout|websocket_closed|event_mismatch|assertion_mismatch|unsupported_capability|...",
        "profile": "lumi_iot_v3_home_controller",
        "implementation": "lumi_iot_v3_home_controller",
        "protocol_pack": "lumi_iot_v3_home_controller@0.1.0",
        "evidence_tier": "mock_contract|mock_conformance|real_preflight|real_e2e",
        "retry_count": 0,
        "normalized": {"kind":"state|event|dispatch|health", "value":{}},
        "evidence": [
          {"kind":"http_response|ws_frame|normalized_state|normalized_event", "ref":"...", "bytes":123, "redacted":true}
        ]
      }
    }

Raw body/frame lưu dạng bounded artifact reference; không nhúng unlimited body
vào event JSONL. Redaction tối thiểu gồm auth/cookie/token và v3
network_data.dev_key, nwk_key, app_key. Evidence collector phải ghi truncated
nếu vượt giới hạn, không im lặng cắt dữ liệu.

run.json, test-results.json, HTML, JUnit và events.jsonl tiếp tục được tạo theo
CLI hiện tại khi bật flag. Report legacy có thể map blocked/error về legacy
failure để không vỡ consumer cũ, nhưng structured e2e.status là nguồn đọc
chính cho E2E và phải hiển thị category/evidence tier. JUnit không được đánh
dấu blocked là <failure>; dùng <skipped message="blocked: ..."> hoặc extension
tương đương đã được test contract chấp thuận.

## 7. Boundary runtime

Các component và trách nhiệm:

1. Profile resolver: đọc profile tương đối với flow, resolve env/secret
   reference, kiểm tra identity/version/endpoint/capability và redaction rule.
2. Capability preflight: tính capability union của case và profile; probe
   health/WS tối thiểu theo case; trả blocker trước khi gửi control.
   Với `hcControl`, metadata command hiện có chỉ biểu diễn capability union
   nên `device_state` và `realtime_cell_readback` cùng được quảng bá như hai
   đường readback có thể dùng; preflight runtime phải áp dụng điều kiện
   `device_state OR realtime_cell_readback`, không được coi danh sách union
   này là phép AND. Khi metadata schema có trường alternatives riêng, union
   này phải được nâng cấp thành một nhóm `anyOf` tường minh.
3. HC HTTP adapter: chỉ biết transport, timeout, safe GET retry, raw
   request/response và HTTP error. Không biết alias semantic.
4. HC WebSocket adapter: connect, subscribe, frame decode, RPC correlation,
   bounded wait, reconnect/resubscribe và close/parse error. Không tự replay.
5. Protocol-pack mapping: khai báo route, DTO shape, ID codec, slot/index rule,
   error map và capability; đây là nơi duy nhất chứa v3 wire details.
6. Canonical normalizer: map payload đã decode thành Entity/SlotState/Event,
   bảo toàn raw và không đoán alias.
7. Evidence collector: bounded raw/normalized artifacts, timestamp nhận,
   redaction và reference.
8. Result classifier: áp dụng bảng status ở trên; không để adapter tự ý quyết
   định product assertion.
9. E2E session: giữ profile, target entity, HTTP client, WS connection,
   subscribed channels, normalized cache và correlation map cho một run.

App driver chỉ phục vụ PlatformDriver và App command. Không đặt HttpClient,
Cell state, WS subscription hoặc HC-specific result vào App driver/context.
TestContext có thể được dùng cho vars/base path/output, nhưng E2E session state
phải có type/ownership riêng.

### Command model đã chọn

Chọn TestCommand::V3(V3Command) (tên enum cụ thể tùy implementation) để
parser/list/index vẫn có một command stream, trong khi payload và dispatch của
HC nằm trong domain module riêng. Đây là nhánh riêng về ownership, không phải
một tập route mới nhúng trong TestCommand chung.

Đã loại bỏ các phương án:

- Thêm từng hc* variant trực tiếp vào enum và match trong App executor: dễ làm
  phình executor, buộc App driver nhận state HC và khiến backward compatibility
  khó kiểm soát.
- Tạo runner YAML thứ hai cho E2E: tạo parser/list/index/report/variable
  semantics thứ hai và phá yêu cầu dùng chung CLI pipeline.
- Hard-code /api/... payload và alias power trong semantic command: làm semantic
  case không tái sử dụng được và trái với gap slot/index hiện tại.
- Coi HTTP 200 {status:true} là applied: source chỉ xác nhận dispatch; sẽ tạo
  false pass khi device/provider không ACK.

## 8. Thiết kế mock/contract/conformance

### Unit

- parse header platform, journey, profile, timeout, tags; giữ App YAML cũ;
- resolve profile tương đối flow và reject path/secret semantic không hợp lệ;
- parse decimal ID qua boundary, bảo toàn string và reject precision loss;
- canonical map DeviceEntity, vector state, state event, config/event shape;
- verify no implicit alias/index inference;
- classifier cho bốn status, redaction và byte/frame bound.

### HTTP contract fixture

Fixture deterministic phải bao phủ health, collection response, state vector,
control request/response, {code,msg}, malformed JSON, wrong type, status code,
timeout và route trailing-slash discrepancy. Fixture phải giữ distinction giữa
device_id URL decimal và cell_id JSON string.

### WebSocket contract fixture

Fixture phải phát đúng subscribe response, cell_runtime_data state/config/event,
device-manager event (chỉ để pack conformance, không smoke baseline), RPC
snapshot/error, malformed frame, close, delayed frame, dropped/replayed absence,
reconnect/resubscribe và snapshot fallback. Fixture phải chứng minh RPC
request_id được match, còn control/event không tự có correlation.

### Protocol-pack conformance

Pack v3 chỉ runnable khi các capability đã khai báo pass: health, discovery,
state decode, control encode, state/event normalize, WS subscribe/RPC,
reconnect policy, invalid payload và evidence redaction. Không khai báo group,
scene, bridge scan, serial hoặc MQTT cho baseline này. Conformance mock pass và
real-lab pass là hai evidence tier riêng.

### Smoke không cần lab thật

Flow mock kiểm tra health → discovery → chọn một entity → state read → subscribe
cell → explicit-index control → event hoặc snapshot/state readback. Khi không có
real endpoint, preflight result là blocked; không chạy mock rồi gắn nhãn
real_e2e.

## 9. Failure policy tối thiểu

- missing profile/field, missing capability, missing endpoint, pack mismatch:
  blocked/preflight;
- HTTP network failure/connection refused sau preflight: error/not_connected;
- HTTP 4xx/5xx body hợp lệ cho positive operation: failed/http_error, giữ
  status/code/msg; HTTP malformed body: error/invalid_payload;
- GET timeout: retry bounded theo profile rồi error/timeout; GET không có
  response hợp lệ không thể là product fail;
- control POST không retry mặc định; nếu response {status:true} nhưng state
  không match trong timeout: failed/assertion_mismatch;
- malformed WebSocket outgoing/incoming frame, parse error, close không recover:
  error/protocol_error hoặc error/websocket_closed;
- event hợp lệ nhưng entity/index/value không match: failed/event_mismatch;
- event bị lỡ sau reconnect nhưng snapshot/state match: control có thể pass với
  evidence readback; assertion event riêng vẫn fail;
- numeric ID vượt JavaScript safe integer: không convert number; giữ decimal
  string và nếu fixture chỉ cung cấp lossy number thì error/invalid_payload;
- không có lab: blocked/no_lab, mock evidence vẫn được lưu và gắn tier riêng;
- cancellation phải đóng WS, dừng retry/poll và finalize evidence bounded.

## 10. Conservative decisions và open caveat

Các quyết định dưới đây được Planner chấp thuận ngày 2026-10-18 theo
`REPORT.md` QA Round 3. Chúng khóa hành vi Phase 1 mà không sửa source v3:

1. Discovery dùng route canonical thực tế `/api/devices`. Adapter/mock phải dùng
   đúng path này; không coi `/api/devices/` là alias tương đương vì HC thực tế
   trả 404 cho path có slash cuối.
2. `idx` canonical là 0-based theo `At0micIdx`, vector storage và WebSocket
   snapshot. Postman example 1-based là mismatch; không auto-convert và không
   suy alias từ tên capability. Mapping phải dùng explicit wire index/pack rule.
3. HTTP control không có ACK/correlation đáng tin cậy: không retry POST mặc định;
   sau dispatch, readback match theo entity + explicit `idx` + value trong
   bounded deadline qua state hoặc snapshot. Không coi `{status:true}` là
   physical apply.
4. Smoke control có thể `passed` bằng state hoặc `CellSnapshot` fallback nếu
   required readback match; `wsExpect` là assertion riêng. Không giả event hoặc
   biến state fallback thành bằng chứng event.
5. `websocketUrl` là profile field bắt buộc và độc lập với `baseUrl`. Thiếu field
   hoặc endpoint thì preflight `blocked`; không tự suy scheme/port.

Open caveat còn lại là evidence runtime/contract fixture phải giữ mapping index
của target fixture. Route discovery đã được xác nhận bằng HC thật và contract
test; không được giải quyết bằng auto-conversion.

## 11. Nguồn đã audit

- SRS: D:\nl-tester\2026-10-05-lumi-e2e-hc-srs.md.
- Kế hoạch: D:\nl-tester\docs\superpowers\plans\2026-10-18-lumi-e2e-device-control.md.
- Lumi Tester: AGENTS.md, lumi-tester/src/parser/{types.rs,yaml.rs},
  src/runner/{mod.rs,context.rs,executor.rs,state.rs,events.rs},
  src/report/{types.rs,json.rs,junit.rs,html.rs}, Cargo.toml,
  docs/{architecture.md,flows/test_execution_flow.md,writing_tests.md}.
- v3 transport: bin/home-controller/src/transport/http/{mod.rs,common.rs,
  health.rs,device.rs,ws.rs} và Postman collection.
- v3 protocol: protocol/src/{dto/realtime.rs,models/cell.rs,models/device.rs,
  models/profile.rs,entities/device.rs,entities/mod.rs} và
  protocol/docs/core-spec/v0.2.0/protocol-core-spec-0.2.0.md.
- v3 runtime/docs: packages/hub-domain/src/services/{devices.rs,pubsub.rs},
  packages/iot-runtime/src/tasks/device_set.rs,
  packages/adapters/iot-kv-adapter/src/repositories/device.rs,
  docs/protocols/realtime-websocket.md.

Các project HC khác không được dùng để lấp gap route, payload, event hoặc
mapping.
