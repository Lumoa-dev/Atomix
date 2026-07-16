# Atomix SDK 设计文档

> 版本: v0.1 (需求框架)
> 最后更新: 2026-07-16
> 依赖: ATXP 协议 v0.4 (`docs/通信协议.md`)
> 交付: `atomix-sdk` (Rust crate) + `atomix-sdk` (Python package)

---

## 1. 概述

Atomix SDK 是宿主项目（Python/Rust/其他语言）与 Atomix Runner 之间的桥梁。它封装了 ATXP 协议的全部细节，提供简洁的 API 用于任务提交、执行跟踪和结果获取。

### 1.1 核心设计原则

- **统一 API 语义**：Python 和 Rust SDK 提供相同的概念和调用模式
- **传输层自动选择**：本地默认走共享内存，远程走 TCP，调用方无需关心底层
- **三种产出模式**：POLL（轮询）/ CALLBACK（webhook）/ STREAM（实时推送）
- **源码+二进制双模提交**：可以提交 .atx 源码让 Runner 编译，也可以提交预编译的 .atxe
- **零依赖启动**：本地模式下无需配置，自动发现同机 Runner

### 1.2 使用场景

| 场景 | 宿主项目 | SDK 语言 | 传输层 |
|------|---------|---------|--------|
| Arche 平台提交任务 | Arche (Python) | Python SDK | TCP（远程或本地） |
| Rust 项目内嵌执行 | Rust 应用 | Rust SDK | 共享内存（本地） |
| CI/CD 流水线 | GitHub Actions | Python SDK | TCP（远程） |
| 本地开发测试 | 开发者机器 | 任一 SDK | 共享内存（本地） |

---

## 2. 通用 API 设计

两个 SDK 共享相同的概念模型，仅语言实现细节不同。

### 2.1 核心类型

```
AtomixClient        连接句柄
  ├── connect_local()            → 自动发现并连接本地 Runner
  ├── connect_remote(addr)       → 连接远程 Runner
  ├── submit(source/binary)      → 提交任务 → 返回 TaskHandle
  ├── submit_and_wait(...)       → 提交并阻塞等待完成
  ├── task(task_id)              → 获取已有任务的句柄
  └── close()                    → 断开连接

TaskHandle          任务句柄
  ├── task_id                    → 任务 ID
  ├── status()                   → 查询任务状态
  ├── wait(timeout?)             → 阻塞等待完成
  ├── output()                   → 获取产出 (POLL 模式)
  ├── on_complete(callback)      → 注册完成回调 (CALLBACK/STREAM 模式)
  ├── stats()                    → 查询执行统计
  └── cancel()                   → 取消任务

TaskOutput          任务产出
  ├── status                     → DONE / ERROR / TIMEOUT
  ├── output                     → OUT zone 数据 (bytes/JSON)
  ├── stats                      → TaskStats
  ├── files                      → 产出文件列表
  └── metadata                   → 回传元数据
```

### 2.2 连接模式

```
# 本地模式 - 自动发现
client = AtomixClient.connect_local()
# SDK 自动:
#   1. 查找共享内存 "atomix-debug-shm-{pid}"
#   2. 查找通知 eventfd
#   3. 建立连接

# 远程模式 - 显式指定
client = AtomixClient.connect_remote("192.168.1.100:9000")
# 或带 TLS:
client = AtomixClient.connect_remote("192.168.1.100:9000", tls=True)
# 或带认证:
client = AtomixClient.connect_remote("192.168.1.100:9000", auth_token="...")
```

### 2.3 提交模式

```
# 源码提交 - Runner 负责编译
task = client.submit(
    source="""
    TASK cleanup {
        RAW = INPUT.file("data.csv")
        CALL parse(RAW)
        OUT -> result
    }
    """,
    task_name="cleanup",
    output_mode="callback",         # POLL / CALLBACK / STREAM
    callback_url="https://my.app/api/callback",
)

# 二进制提交 - 预编译产物
task = client.submit(
    binary=open("cleanup.atxe", "rb").read(),
    task_name="cleanup",
    output_mode="poll",
)

# 从文件提交 - SDK 自动判断模式
task = client.submit_file("tasks/cleanup.atx")   # .atx → SOURCE
task = client.submit_file("tasks/cleanup.atxe")  # .atxe → BINARY
```

### 2.4 产出获取

```python
# 方式 1: 阻塞等待
result = task.wait(timeout_ms=60000)  # 60 秒超时
print(result.output)                  # OUT 数据
print(result.stats.total_instrs)      # 执行统计

# 方式 2: POLL 轮询
while not task.is_done():
    time.sleep(1)
output = task.output()

# 方式 3: CALLBACK 回调
def on_done(output):
    print(f"Task {output.task_id} done: {output.output}")
task.on_complete(on_done)

# 方式 4: STREAM 流式
for chunk in task.stream():
    print(f"Progress: {chunk}")
```

---

## 3. Python SDK 设计

### 3.1 包信息

```
包名:     atomix-sdk
版本:     0.1.0
Python:   >=3.10
依赖:     protobuf, zstandard (可选)
安装:     pip install atomix-sdk
源目录:   atomix_sdk/
```

### 3.2 模块结构

```
atomix_sdk/
├── __init__.py          # 公开 API: AtomixClient, TaskHandle, TaskOutput
├── client.py            # AtomixClient 实现
├── task.py              # TaskHandle 实现
├── transport/           # 传输层抽象
│   ├── __init__.py
│   ├── base.py          # Transport ABC
│   ├── local.py         # 共享内存传输 (mmap + eventfd)
│   └── remote.py        # TCP 传输 (socket + Protobuf)
├── protocol/            # ATXP 协议实现
│   ├── __init__.py
│   ├── frame.py         # 帧头序列化/反序列化
│   ├── messages.py      # Protobuf 消息封装
│   └── atxp_pb2.py      # 生成的 Protobuf 代码
├── errors.py            # 异常定义
└── types.py             # 数据类型定义
```

### 3.3 API 参考

```python
class AtomixClient:
    """Atomix 客户端连接句柄"""

    @staticmethod
    def connect_local(
        runner_pid: Optional[int] = None,
    ) -> "AtomixClient":
        """
        自动发现并连接本地 Runner。

        自动发现流程:
        1. 如果指定 runner_pid → 直接连接
        2. 扫描 /dev/shm/atomix-* (Linux) 或检查 Named FileMapping (Windows)
        3. 选择 state=READY 的 Runner
        4. 建立共享内存连接
        """

    @staticmethod
    def connect_remote(
        addr: str,
        tls: bool = False,
        auth_token: Optional[str] = None,
        tenant_id: Optional[str] = None,
        tls_ca_cert: Optional[str] = None,
    ) -> "AtomixClient":
        """
        连接远程 Runner。

        参数:
        - addr: "host:port" 格式
        - tls: 是否启用 TLS
        - auth_token: 认证令牌
        - tenant_id: 租户标识
        - tls_ca_cert: 自定义 CA 证书路径
        """

    def submit(
        self,
        source: Optional[str] = None,
        binary: Optional[bytes] = None,
        task_name: str = "",
        output_mode: str = "poll",
        callback_url: Optional[str] = None,
        callback_headers: Optional[dict] = None,
        priority: int = 5,
        timeout_ms: int = 0,
        metadata: Optional[dict] = None,
        compile_opt_level: int = 0,
        compile_debug_info: bool = True,
    ) -> "TaskHandle":
        """
        提交任务。

        source 和 binary 至少提供一个。
        如 source 以 '.atx' 结尾 → 视为文件路径, SDK 读取内容。
        """

    def submit_file(self, path: str, **kwargs) -> "TaskHandle":
        """从文件提交任务。自动判断 .atx (SOURCE) 或 .atxe (BINARY)。"""

    def submit_and_wait(self, *args, timeout_ms: int = 60000, **kwargs) -> "TaskOutput":
        """提交并阻塞等待完成。"""

    def task(self, task_id: str) -> "TaskHandle":
        """获取已有任务的句柄 (用于 POLL 模式重连后恢复)。"""

    def close(self):
        """断开连接。"""


class TaskHandle:
    """任务句柄"""

    task_id: str

    def status(self) -> "TaskStatus":
        """查询任务当前状态 (Query runner/tasks/{tid}/status GET)"""

    def is_done(self) -> bool:
        """任务是否已完成 (DONE/ERROR/TIMEOUT)"""

    def wait(self, timeout_ms: int = 60000) -> "TaskOutput":
        """
        阻塞等待任务完成。

        - POLL 模式: 轮询 output 端点 (每 500ms)
        - STREAM 模式: 等待 TaskOutput 事件
        - CALLBACK 模式: 不支持 wait() (抛出异常)
        """

    def output(self) -> "TaskOutput":
        """获取任务产出 (POLL 模式, 非阻塞)"""

    def on_complete(self, callback: Callable[["TaskOutput"], None]):
        """注册完成回调 (CALLBACK/STREAM 模式)"""

    def stats(self) -> "TaskStats":
        """查询执行统计"""

    def cancel(self):
        """取消任务 (Command runner/tasks/{tid}/status SET)"""

    def stream(self) -> Iterator["TaskOutput"]:
        """流式获取产出 (STREAM 模式)"""


@dataclass
class TaskOutput:
    task_id: str
    status: str              # "DONE" | "ERROR" | "TIMEOUT"
    output: bytes
    error: str
    stats: TaskStats
    files: List[OutputFile]
    started_at: int          # ns
    completed_at: int        # ns
    metadata: dict


@dataclass
class TaskStats:
    total_instrs: int
    ecall_count: int
    blocking_count: int
    oom_count: int
    quantum_count: int
    context_switches: int
    exception_count: int
    wall_time_ns: int
    cpu_time_ns: int
    peak_mem: int
    current_mem: int
    io_read_bytes: int
    io_write_bytes: int
    net_rx_bytes: int
    net_tx_bytes: int


@dataclass
class OutputFile:
    name: str
    path: str
    size: int
    mime_type: str
    content: bytes   # None 表示需要从 Runner 下载
```

### 3.4 使用示例（在 Arche 中集成）

```python
# arche/Services/plugins/my_plugin/tasks.py

from atomix_sdk import AtomixClient
import os

class AtomixTaskRunner:
    """Arche 插件中封装 Atomix 任务执行"""

    def __init__(self):
        # 生产环境: 远程 Runner
        if os.environ.get("ATOMIX_REMOTE"):
            self.client = AtomixClient.connect_remote(
                addr=os.environ["ATOMIX_ADDR"],
                tls=True,
                auth_token=os.environ["ATOMIX_TOKEN"],
                tenant_id="arche",
            )
        else:
            # 开发环境: 本地 Runner
            self.client = AtomixClient.connect_local()

    def run_cleanup(self, file_path: str) -> dict:
        """提交清理任务并等待结果"""

        source = f"""
        TOOLS {{
            FUNC parse(str RAW) -> dict {{
                // ... 解析逻辑
            }}
        }}

        TASK cleanup {{
            RAW = INPUT.file("{file_path}")
            CALL parse(RAW)
            OUT -> result
        }}
        """

        output = self.client.submit_and_wait(
            source=source,
            task_name="cleanup",
            output_mode="poll",
            timeout_ms=30000,
        )

        if output.status == "DONE":
            import json
            return json.loads(output.output)
        else:
            raise RuntimeError(f"Task failed: {output.error}")
```

---

## 4. Rust SDK 设计

### 4.1 Crate 信息

```
crate:   atomix-sdk
版本:    0.1.0
edition: 2021
依赖:    tokio, prost, zstd (可选)
源目录:  crates/atomix-sdk/
```

### 4.2 模块结构

```
crates/atomix-sdk/
├── Cargo.toml
└── src/
    ├── lib.rs              # 公开 API: AtomixClient, TaskHandle, TaskOutput
    ├── client.rs           # AtomixClient 实现
    ├── task.rs             # TaskHandle 实现
    ├── transport/          # 传输层
    │   ├── mod.rs
    │   ├── local.rs        # 共享内存: shm_open + mmap + eventfd
    │   └── remote.rs       # TCP: tokio::net::TcpStream + TLS
    ├── protocol/           # ATXP 协议
    │   ├── mod.rs
    │   ├── frame.rs        # 帧头序列化 (手写二进制)
    │   ├── messages.rs     # Protobuf 消息类型别名
    │   └── atxp.rs         # prost 生成的代码
    ├── error.rs            # 错误类型
    └── types.rs            # 数据类型
```

### 4.3 API 参考

```rust
use atomix_sdk::{AtomixClient, TaskHandle, TaskOutput, TaskStats};
use std::collections::HashMap;
use std::time::Duration;

/// Atomix 客户端连接句柄
pub struct AtomixClient { /* ... */ }

impl AtomixClient {
    /// 连接本地 Runner（共享内存）
    pub async fn connect_local() -> Result<Self> { /* ... */ }

    /// 连接远程 Runner（TCP）
    pub async fn connect_remote(
        addr: &str,
        tls: Option<TlsConfig>,
        auth_token: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<Self> { /* ... */ }

    /// 提交任务
    pub async fn submit(&self, req: SubmitRequest) -> Result<TaskHandle> { /* ... */ }

    /// 提交并阻塞等待完成
    pub async fn submit_and_wait(
        &self,
        req: SubmitRequest,
        timeout: Duration,
    ) -> Result<TaskOutput> { /* ... */ }

    /// 获取已有任务句柄
    pub fn task(&self, task_id: &str) -> TaskHandle { /* ... */ }

    /// 关闭连接
    pub async fn close(self) -> Result<()> { /* ... */ }
}

/// 提交请求
pub struct SubmitRequest {
    pub source: Option<String>,           // .atx 源码
    pub binary: Option<Vec<u8>>,          // .atxe 二进制
    pub task_name: String,
    pub output_mode: OutputMode,          // Poll / Callback / Stream
    pub callback_url: Option<String>,
    pub callback_headers: Option<HashMap<String, String>>,
    pub priority: u32,                    // 0-10, 默认 5
    pub timeout_ms: u64,                  // 0=无限制
    pub metadata: Option<HashMap<String, String>>,
    pub compile_opt_level: u32,           // 0-3
    pub compile_debug_info: bool,
}

/// 产出模式
pub enum OutputMode {
    Poll,
    Callback,
    Stream,
}

/// 任务句柄
pub struct TaskHandle { /* ... */ }

impl TaskHandle {
    pub fn task_id(&self) -> &str { /* ... */ }

    /// 查询状态
    pub async fn status(&self) -> Result<TaskStatus> { /* ... */ }

    /// 是否完成
    pub async fn is_done(&self) -> Result<bool> { /* ... */ }

    /// 阻塞等待完成
    pub async fn wait(&self, timeout: Duration) -> Result<TaskOutput> { /* ... */ }

    /// 获取产出 (非阻塞)
    pub async fn output(&self) -> Result<Option<TaskOutput>> { /* ... */ }

    /// 注册完成回调 (需要 tokio channel)
    pub async fn on_complete<F>(&self, callback: F) -> Result<()>
    where F: FnOnce(TaskOutput) + Send + 'static { /* ... */ }

    /// 查询统计
    pub async fn stats(&self) -> Result<TaskStats> { /* ... */ }

    /// 取消任务
    pub async fn cancel(&self) -> Result<()> { /* ... */ }
}

/// 任务产出
#[derive(Debug, Clone)]
pub struct TaskOutput {
    pub task_id: String,
    pub status: OutputStatus,         // Done / Error / Timeout
    pub output: Vec<u8>,
    pub error: String,
    pub stats: TaskStats,
    pub files: Vec<OutputFile>,
    pub started_at: u64,
    pub completed_at: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum OutputStatus {
    Done,
    Error,
    Timeout,
    Running,
}
```

### 4.4 使用示例

```rust
use atomix_sdk::{AtomixClient, SubmitRequest, OutputMode};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 连接本地 Runner
    let client = AtomixClient::connect_local().await?;

    // 提交任务
    let task = client.submit(SubmitRequest {
        source: Some(include_str!("../tasks/cleanup.atx").to_string()),
        task_name: "cleanup".into(),
        output_mode: OutputMode::Poll,
        priority: 5,
        ..Default::default()
    }).await?;

    println!("Task submitted: {}", task.task_id());

    // 等待完成
    let output = task.wait(Duration::from_secs(30)).await?;
    match output.status {
        OutputStatus::Done => {
            println!("Output: {:?}", String::from_utf8_lossy(&output.output));
            println!("Stats: {:?}", output.stats);
        }
        OutputStatus::Error => {
            eprintln!("Task error: {}", output.error);
        }
        OutputStatus::Timeout => {
            eprintln!("Task timed out");
        }
        _ => {}
    }

    Ok(())
}
```

---

## 5. 传输层自动选择

SDK 的核心便利特性：连接时自动选择最优传输方式。

### 5.1 选择逻辑

```
connect_local() → 共享内存
  ↓ 失败
connect_local() → Unix Domain Socket (备用)
  ↓ 失败
connect_local() → TCP localhost:9000 (最后备用)

connect_remote(addr) → TCP
  ├── 同机 IP? → 优先走本地 (如果 Runner 允许)
  └── 远程 IP → TCP (含 TLS 可选)
```

### 5.2 本地发现机制

```
Linux:
  1. 扫描 /dev/shm/atomix-*-ctrl (共享内存区域)
  2. 检查每个区域的控制区 magic + state
  3. 选择 state=READY 的 Runner

Windows:
  1. 枚举 Named FileMapping: "atomix-*"
  2. 同样检查 magic + state

Unix Domain Socket (fallback):
  1. 检查 /tmp/atomix.sock
  2. 如果存在 → Unix Socket 连接
```

---

## 6. 错误处理

### 6.1 异常层次（Python）

```
AtomixError (基类)
├── ConnectionError          # 无法连接 Runner
│   ├── LocalRunnerNotFound  # 本地 Runner 未启动
│   └── RemoteRefused         # 远程连接被拒绝
├── AuthError                # 认证失败 (401)
├── SubmitError              # 提交失败
│   ├── CompileError         # 源码编译失败 (COMPILE_ERROR)
│   └── RejectedError        # 被拒绝 (资源不足、deny_commands)
├── TimeoutError             # 操作超时 (408)
└── TaskError                # 任务执行失败
    ├── TaskRuntimeError     # VM 异常
    └── TaskTimeoutError     # 任务执行超时
```

### 6.2 错误类型（Rust）

```rust
#[derive(Debug, thiserror::Error)]
pub enum AtomixError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("auth failed: {0}")]
    Auth(String),

    #[error("submit rejected: {0}")]
    SubmitRejected(String),

    #[error("compile error: {0:?}")]
    CompileError(Vec<CompileError>),

    #[error("timeout")]
    Timeout,

    #[error("task error: {0}")]
    Task(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("internal error: {0}")]
    Internal(String),
}
```

---

## 7. 线程安全与异步

### 7.1 Python SDK

- `AtomixClient`：线程安全，可在多个线程间共享
- `TaskHandle.wait()`：阻塞调用，释放 GIL（共享内存 IO 在 C 级别）
- 回调模式：在独立线程中调用 callback
- 异步支持：提供 `AsyncAtomixClient` (基于 asyncio)，接口完全一致

### 7.2 Rust SDK

- `AtomixClient`：`Send + Sync`，可在多线程间共享
- 所有网络 IO 基于 `tokio` 异步
- 共享内存 IO 使用 `tokio::task::spawn_blocking` 避免阻塞 event loop
- `TaskHandle`：`Send + Sync`，可安全发送到其他 task

---

## 8. 依赖最小化

### Python SDK

```
install_requires = [
    "protobuf>=4.0",       # Protobuf 运行时
    "typing-extensions",   # 类型标注增强
]
extras = {
    "compression": ["zstandard>=0.20"],  # Submit 压缩传输 (可选)
}
```

最小安装仅需 `protobuf`。压缩为可选依赖。

### Rust SDK

```toml
[dependencies]
tokio = { version = "1", features = ["net", "sync", "time"] }
prost = "0.12"
bytes = "1"

# 可选
zstd = { version = "0.13", optional = true }

[features]
default = []
compression = ["zstd"]
```

---

## 9. 与其他语言的互操作

虽然只维护 Python 和 Rust SDK，但 ATXP 协议是完全开放的——任何语言都可以通过以下方式集成：

### 9.1 最小可行集成（任意语言）

```
1. 获取 atxp.proto → 用本语言的 Protobuf 工具生成代码
2. 实现帧头序列化 (16 字节固定格式, 含 CRC-16-CCITT, 手写)
3. 实现 Submit → SubmitResult → OutputRequest → TaskOutput 消息流
4. TCP 连接即可 (不需要实现共享内存)
```

### 9.2 参考实现优先级

| 语言 | 优先级 | 说明 |
|------|--------|------|
| Python + Rust | **一级** | 官方维护，完整功能 |
| JavaScript/TypeScript | 社区 | 可基于 proto 文件生成 |
| Go | 社区 | gRPC 生态类似 |
| Java/Kotlin | 社区 | Maven 中央仓库自行发布 |

---

> 本文档为 Atomix SDK 的**需求框架**。具体实现代码（共享内存绑定、TCP 连接池、回调重试逻辑等）在开发阶段细化。
