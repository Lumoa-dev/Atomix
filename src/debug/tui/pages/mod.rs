//! TUI 页面系统 — Page trait 和 18 个本地页面的注册表。
//!
//! 对应设计文档 §3（页面体系）。

mod home;
mod viewer_pages;
mod info_pages;
mod detail_pages;
mod viz_pages;

use std::collections::HashMap;
use ratatui::Frame;
use ratatui::layout::Rect;
use crate::debug::session::LocalDebugSession;

// ─── 页面 ID ──────────────────────────────────────────────

/// 所有本地页面的唯一标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PageId {
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
    // 远程页面
    #[allow(dead_code)]
    RemoteConnections,
    #[allow(dead_code)]
    RemoteDashboard,
    #[allow(dead_code)]
    RemoteTaskList,
    #[allow(dead_code)]
    RemoteTaskSnapshot,
    #[allow(dead_code)]
    RemoteController,
    #[allow(dead_code)]
    RemoteSlots,
    #[allow(dead_code)]
    RemoteSubmit,
    #[allow(dead_code)]
    RemoteConfig,
    #[allow(dead_code)]
    RemoteTaskPool,
    #[allow(dead_code)]
    RemoteLogs,
    #[allow(dead_code)]
    RemoteSlotsAnim,
    #[allow(dead_code)]
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
    fn on_key_shortcut(&mut self, _session: &mut LocalDebugSession, _key: char, _status: &mut String) {}

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

    /// 注册所有页面。
    pub fn register_all(&mut self, session: &LocalDebugSession) {
        self.pages.insert(PageId::Home, Box::new(home::HomePage::new(session)));
        self.pages.insert(PageId::SourceView, Box::new(viewer_pages::SourceViewPage::new(session)));
        self.pages.insert(PageId::BinaryView, Box::new(viewer_pages::BinaryViewPage::new(session)));
        self.pages.insert(PageId::DisasmView, Box::new(viewer_pages::DisasmViewPage::new(session)));
        self.pages.insert(PageId::RegsMemory, Box::new(viewer_pages::RegsMemPage::new(session)));
        self.pages.insert(PageId::CallStack, Box::new(info_pages::CallStackPage::new(session)));
        self.pages.insert(PageId::Breakpoints, Box::new(info_pages::BreakpointsPage::new(session)));
        self.pages.insert(PageId::IsContext, Box::new(info_pages::IsContextPage::new(session)));
        self.pages.insert(PageId::SegmentInfo, Box::new(info_pages::SegmentInfoPage::new(session)));
        self.pages.insert(PageId::PerfAnalysis, Box::new(info_pages::PerfAnalysisPage::new(session)));
        self.pages.insert(PageId::ZoneStatus, Box::new(info_pages::ZoneStatusPage::new(session)));
        self.pages.insert(PageId::ExceptionDetail, Box::new(detail_pages::ExceptionDetailPage::new(session)));
        self.pages.insert(PageId::InputDetail, Box::new(detail_pages::InputDetailPage::new(session)));
        self.pages.insert(PageId::OutputDetail, Box::new(detail_pages::OutputDetailPage::new(session)));
        self.pages.insert(PageId::StepDetail, Box::new(detail_pages::StepDetailPage::new(session)));
        self.pages.insert(PageId::DataTimeline, Box::new(viz_pages::DataTimelinePage::new(session)));
        self.pages.insert(PageId::HookTimeline, Box::new(viz_pages::HookTimelinePage::new(session)));
        self.pages.insert(PageId::TaskDependency, Box::new(viz_pages::TaskDependencyPage::new(session)));
        self.pages.insert(PageId::WatchReplay, Box::new(viz_pages::WatchReplayPage::new(session)));
    }

    pub fn get_page(&self, id: &PageId) -> Option<&Box<dyn Page>> {
        self.pages.get(id)
    }

    pub fn get_page_mut(&mut self, id: &PageId) -> Option<&mut Box<dyn Page>> {
        self.pages.get_mut(id)
    }
}
