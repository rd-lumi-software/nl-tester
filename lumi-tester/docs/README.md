# Tài liệu Lumi Tester

Chào mừng đến với tài liệu chính thức của Lumi Tester.
Lumi Tester là công cụ kiểm thử tự động hiệu năng cao, đa nền tảng dành cho ứng dụng Android, iOS và Web.

## 📚 Mục lục Tài liệu

### [Bắt đầu (Setup)](setup.md)
Hướng dẫn cài đặt và cấu hình môi trường Lumi Tester.

### [Kiến trúc Hệ thống](architecture.md)
Tổng quan về các thành phần nội bộ, bao gồm Runner, Driver Abstraction và các Implementations.

### [Hướng dẫn Viết Test](writing_tests.md)
Cách viết kịch bản test hiệu quả, sử dụng selector và tổ chức file.

### [Tra cứu Lệnh (Command Reference)](api/commands.md)
Chi tiết đầy đủ về tất cả các lệnh hỗ trợ (`tap`, `see`, `swipe`, `assert`, v.v.).

### [Kiểm thử phần cứng bằng Camera](camera-hardware-testing.md)
Dùng camera RTSP đọc trạng thái LED của thiết bị vật lý (công tắc, ổ cắm, home
controller…) để verify phần cứng: lệnh `camera calibrate/detect` và các lệnh YAML
`assertDeviceState`, `waitDeviceState`, `assertDeviceTransition`, `waitLedPattern`,
`getDeviceState`.

### [Tự động hóa Phần cứng (Hardware Automation)](hardware-control.md)
Tự động hóa gạt công tắc vật lý (Servo), điều khiển nguồn (Relay) và đọc trạng thái màu sắc/chớp tắt LED (Color Sensor) trực tiếp bằng 100% Native Rust qua truyền thông RS485/Serial.

### [Tích hợp Kỹ năng AI (AI Skills & MCP)](ai-skills-integration.md)
Hướng dẫn tích hợp kỹ năng kiểm thử tự động Lumi Tester vào Google Antigravity, OpenAI Codex, Claude Code, Cursor và cấu hình Lumi Tester MCP Server.

### [HC Agent & CLI Guide](hc-agent-cli-guide.md)
Hướng dẫn tiếng Việt toàn diện cho hai đối tượng: (1) người dùng gửi yêu cầu kiểm thử HC bằng ngôn ngữ tự nhiên cho AI agent; (2) developer chạy thư viện HC test có sẵn từ CLI. Bao gồm sơ đồ kiến trúc Mermaid, intake gate, handoff plan, CLI command sequence, bảng debug failure, và lộ trình mở rộng App → Cloud → HC.

### [Luồng thực thi Test](flows/test_execution_flow.md)
Hiểu về vòng đời của một phiên test, từ lúc đọc file YAML đến khi tương tác thiết bị.

### [Triển khai Driver](api/drivers.md)
Chi tiết kỹ thuật về cách các driver Android, iOS, Web được hiện thực hóa.

## 🤝 Đóng góp

Chúng tôi hoan nghênh mọi đóng góp! Vui lòng xem `architecture.md` để hiểu codebase trước khi gửi Pull Request.
