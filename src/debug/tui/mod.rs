//! TUI 调试器 — 基于 ratatui 的终端界面调试器。
//!
//! 对应设计文档 §2（TUI 布局）、§3（页面体系）、§4（命令体系）。
//!
//! # 布局
//!
//! ```text
//! ┌─ 标题栏 ────────────────────────────────────────────────────┐
//! │  atomix debug — <文件名>                                     │
//! ├─ 面包屑 ────────────────────────────────────────────────────┤
//! │  Home ▸ STEP 2: validate                                     │
//! ├─ 左侧主视图（≈ 70%）──┬─ 右侧状态面板 ──────────────────────┤
//! │                        │  IS* Context（持久）                 │
//! │                        │  ───────────────                     │
//! │  页面内容（18 页面之一） │  Variables / Watch                  │
//! │                        │  ───────────────                     │
//! │                        │  动态面板（用户切换）                  │
//! ├─ 命令栏 ──────────────┴──────────────────────────────────────┤
//! │  > command                                                   │
//! ├─ help（help 命令时弹出）─────────────────────────────────────┤
//! │  键盘快捷键 & 命令参考                                        │
//! └──────────────────────────────────────────────────────────────┘
//! ```

pub mod app;
pub mod layout;
pub mod pages;

use crate::debug::session::LocalDebugSession;
use crate::debug::tui::app::TuiApp;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

/// 启动 TUI 调试器。
///
/// 接管终端，进入全屏 TUI 模式。退出后恢复终端状态。
pub fn run_tui(session: LocalDebugSession) -> Result<(), String> {
    // 如果尚未收集轨迹，自动收集
    let mut session = session;
    if !session.collected {
        session.collect_trace();
    }

    // 初始化终端
    enable_raw_mode().map_err(|e| format!("无法启用 raw mode: {}", e))?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .map_err(|e| format!("无法进入 alternate screen: {}", e))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| format!("无法创建终端: {}", e))?;

    let app_result = {
        let mut app = TuiApp::new(session);
        app.run(&mut terminal)
    };

    // 恢复终端
    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen)
        .map_err(|e| format!("无法离开 alternate screen: {}", e))?;
    disable_raw_mode().map_err(|e| format!("无法禁用 raw mode: {}", e))?;

    app_result
}
