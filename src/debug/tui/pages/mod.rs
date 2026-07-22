//! TUI 页面系统 — Page trait 和 18+12 个页面的注册表。
//!
//! 对应设计文档 §3（本地页面体系）、§7.1（远程页面）。

mod detail_pages;
mod home;
mod info_pages;
mod viewer_pages;
mod viz_pages;

use crate::debug::session::LocalDebugSession;
use crate::debug::tui::remote::{
    ConfigPage, ConnectionsPage, ControllerPage, DashboardPage, LogsPage, PoolPage, RemotePerfPage,
    SlotsAnimPage, SlotsPage, SubmitPage, TaskListPage, TaskSnapshotPage,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use std::collections::HashMap;

// ─── 页面 ID ──────────────────────────────────────────────

/// 所有页面的唯一标识（18 本地 + 12 远程 = 30 页面）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PageId {
    // 本地页面 (18)
    Home,
    StepDetail,
    SourceView,
    BinaryView,
    DisasmView,
    RegsMemory,
    CallStack,
    Breakpoints,
    IsContext,
    SegmentInfo,
    PerfAnalysis,
    DataTimeline,
    HookTimeline,
    TaskDependency,
    WatchReplay,
    ExceptionDetail,
    InputDetail,
    OutputDetail,
    ZoneStatus,
    // 远程页面 (12)
    RemoteConnections,
    RemoteDashboard,
    RemoteTaskList,
    RemoteTaskSnapshot,
    RemoteController,
    RemoteSlots,
    RemoteSubmit,
    RemoteConfig,
    RemoteTaskPool,
    RemoteLogs,
    RemoteSlotsAnim,
    RemotePerf,
}

// ─── Page trait ───────────────────────────────────────────

/// 所有 TUI 页面必须实现的接口。
pub trait Page {
    /// 页面显示标题。
    fn title(&self) -> &str;
    /// 渲染页面内容到指定区域。
    fn render(&mut self, frame: &mut Frame, area: Rect, session: &mut LocalDebugSession);
    /// 按 Enter 时的操作（选中/进入子页面）。
    fn on_enter(&mut self, _session: &mut LocalDebugSession, _status: &mut String) {}
    /// 按 + 键缩放。
    fn on_zoom_in(&mut self, _session: &mut LocalDebugSession) {}
    /// 按 - 键缩放。
    fn on_zoom_out(&mut self, _session: &mut LocalDebugSession) {}
    /// 键盘快捷键处理。
    fn on_key_shortcut(
        &mut self,
        _session: &mut LocalDebugSession,
        _key: char,
        _status: &mut String,
    ) {
    }
    /// 数据变化时更新。
    fn on_data_changed(&mut self, _session: &mut LocalDebugSession) {}
}

// ─── 页面注册表 ──────────────────────────────────────────

/// 页面注册表，管理所有页面的创建和访问。
pub struct PageRegistry {
    pages: HashMap<PageId, Box<dyn Page>>,
}

impl PageRegistry {
    pub fn new() -> Self {
        Self {
            pages: HashMap::new(),
        }
    }

    /// 注册所有本地页面。
    pub fn register_all(&mut self, session: &LocalDebugSession) {
        // 本地页面 (18)
        self.pages
            .insert(PageId::Home, Box::new(home::HomePage::new(session)));
        self.pages.insert(
            PageId::SourceView,
            Box::new(viewer_pages::SourceViewPage::new(session)),
        );
        self.pages.insert(
            PageId::BinaryView,
            Box::new(viewer_pages::BinaryViewPage::new(session)),
        );
        self.pages.insert(
            PageId::DisasmView,
            Box::new(viewer_pages::DisasmViewPage::new(session)),
        );
        self.pages.insert(
            PageId::RegsMemory,
            Box::new(viewer_pages::RegsMemPage::new(session)),
        );
        self.pages.insert(
            PageId::CallStack,
            Box::new(info_pages::CallStackPage::new(session)),
        );
        self.pages.insert(
            PageId::Breakpoints,
            Box::new(info_pages::BreakpointsPage::new(session)),
        );
        self.pages.insert(
            PageId::IsContext,
            Box::new(info_pages::IsContextPage::new(session)),
        );
        self.pages.insert(
            PageId::SegmentInfo,
            Box::new(info_pages::SegmentInfoPage::new(session)),
        );
        self.pages.insert(
            PageId::PerfAnalysis,
            Box::new(info_pages::PerfAnalysisPage::new(session)),
        );
        self.pages.insert(
            PageId::ZoneStatus,
            Box::new(info_pages::ZoneStatusPage::new(session)),
        );
        self.pages.insert(
            PageId::ExceptionDetail,
            Box::new(detail_pages::ExceptionDetailPage::new(session)),
        );
        self.pages.insert(
            PageId::InputDetail,
            Box::new(detail_pages::InputDetailPage::new(session)),
        );
        self.pages.insert(
            PageId::OutputDetail,
            Box::new(detail_pages::OutputDetailPage::new(session)),
        );
        self.pages.insert(
            PageId::StepDetail,
            Box::new(detail_pages::StepDetailPage::new(session)),
        );
        self.pages.insert(
            PageId::DataTimeline,
            Box::new(viz_pages::DataTimelinePage::new(session)),
        );
        self.pages.insert(
            PageId::HookTimeline,
            Box::new(viz_pages::HookTimelinePage::new(session)),
        );
        self.pages.insert(
            PageId::TaskDependency,
            Box::new(viz_pages::TaskDependencyPage::new(session)),
        );
        self.pages.insert(
            PageId::WatchReplay,
            Box::new(viz_pages::WatchReplayPage::new(session)),
        );
        // 远程页面 (12)
        self.register_remote_pages();
    }

    /// 注册所有远程页面。
    pub fn register_remote_pages(&mut self) {
        self.pages
            .insert(PageId::RemoteConnections, Box::new(ConnectionsPage::new()));
        self.pages
            .insert(PageId::RemoteDashboard, Box::new(DashboardPage::new()));
        self.pages
            .insert(PageId::RemoteTaskList, Box::new(TaskListPage::new()));
        self.pages.insert(
            PageId::RemoteTaskSnapshot,
            Box::new(TaskSnapshotPage::new()),
        );
        self.pages
            .insert(PageId::RemoteController, Box::new(ControllerPage::new()));
        self.pages
            .insert(PageId::RemoteSlots, Box::new(SlotsPage::new()));
        self.pages
            .insert(PageId::RemoteSubmit, Box::new(SubmitPage::new()));
        self.pages
            .insert(PageId::RemoteConfig, Box::new(ConfigPage::new()));
        self.pages
            .insert(PageId::RemoteTaskPool, Box::new(PoolPage::new()));
        self.pages
            .insert(PageId::RemoteLogs, Box::new(LogsPage::new()));
        self.pages
            .insert(PageId::RemoteSlotsAnim, Box::new(SlotsAnimPage::new()));
        self.pages
            .insert(PageId::RemotePerf, Box::new(RemotePerfPage::new()));
    }

    /// 注册远程页面（不带 session 的重载）。
    pub fn register_remote_only(&mut self) {
        self.register_remote_pages();
        // 远程模式下也注册基本的本地页面（如 Home）
        // 但不需要，远程模式有自己独立的页面集
    }

    pub fn get_page(&self, id: &PageId) -> Option<&Box<dyn Page>> {
        self.pages.get(id)
    }

    pub fn get_page_mut(&mut self, id: &PageId) -> Option<&mut Box<dyn Page>> {
        self.pages.get_mut(id)
    }
}
