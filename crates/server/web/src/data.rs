// crates/server/web/src/data.rs
// 集中式数据层:四个 API 拉取函数 + 页面间共享的单元缓存(DataStore)。
// 页面不再各自 use_future 拉取,改为从 DataStore 读缓存;MainShell 编排预载。
use crate::api::request;
use dioxus::prelude::*;
use std::collections::HashSet;
use submerge_web_core::dto::{CombinedDto, ConfigDto, PreviewResp, SourceDto};

pub async fn fetch_sources(token: Option<&str>) -> Result<Vec<SourceDto>, String> {
    let body = request("GET", "/admin/sources", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_combineds(token: Option<&str>) -> Result<Vec<CombinedDto>, String> {
    let body = request("GET", "/admin/combineds", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

pub async fn fetch_config(token: Option<&str>) -> Result<ConfigDto, String> {
    let body = request("GET", "/admin/config", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKey {
    Sources,
    Combineds,
    Preview,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Idle,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone)]
pub struct CacheState<T> {
    pub status: CacheStatus,
    pub data: Option<T>,
    // 加载失败原因:各页面 error 区展示用。
    pub error: String,
}

impl<T> Default for CacheState<T> {
    fn default() -> Self {
        Self {
            status: CacheStatus::Idle,
            data: None,
            error: String::new(),
        }
    }
}

pub async fn fetch_preview(token: Option<&str>) -> Result<PreviewResp, String> {
    let body = request("GET", "/admin/preview", None, token).await?;
    serde_json::from_str(&body).map_err(|e| format!("解析失败: {}", e))
}

/// 页面共享的单元缓存 + 拉取编排。由 MainShell 经 use_context_provider 提供。
#[derive(Clone, Copy)]
pub struct DataStore {
    pub sources: Signal<CacheState<Vec<SourceDto>>>,
    pub combineds: Signal<CacheState<Vec<CombinedDto>>>,
    pub preview: Signal<CacheState<PreviewResp>>,
    pub config: Signal<CacheState<ConfigDto>>,
    pub token: Signal<Option<String>>,
    in_flight: Signal<HashSet<UnitKey>>,
}

impl DataStore {
    pub fn provide(token: Signal<Option<String>>) -> DataStore {
        use_context_provider(move || DataStore {
            sources: Signal::new(CacheState::default()),
            combineds: Signal::new(CacheState::default()),
            preview: Signal::new(CacheState::default()),
            config: Signal::new(CacheState::default()),
            token,
            in_flight: Signal::new(HashSet::new()),
        })
    }

    pub fn required_units(tab: usize) -> &'static [UnitKey] {
        match tab {
            0 => &[UnitKey::Sources, UnitKey::Preview],   // 概览
            1 => &[UnitKey::Sources],                     // 订阅源
            2 => &[UnitKey::Combineds, UnitKey::Sources], // 组合订阅
            3 => &[UnitKey::Preview, UnitKey::Combineds], // 预览
            _ => &[UnitKey::Config],                      // 配置
        }
    }

    pub fn status_of(&self, key: UnitKey) -> CacheStatus {
        match key {
            UnitKey::Sources => self.sources.read().status,
            UnitKey::Combineds => self.combineds.read().status,
            UnitKey::Preview => self.preview.read().status,
            UnitKey::Config => self.config.read().status,
        }
    }

    /// 目标 tab 所需单元全部 Ready(秒开判断)
    pub fn all_ready(&self, tab: usize) -> bool {
        Self::required_units(tab)
            .iter()
            .all(|k| self.status_of(*k) == CacheStatus::Ready)
    }

    /// 目标 tab 所需单元已全部完成(Ready/Error 都算,无 Loading)
    pub fn all_finished(&self, tab: usize) -> bool {
        Self::required_units(tab)
            .iter()
            .all(|k| self.status_of(*k) != CacheStatus::Loading)
    }

    /// 目标 tab 是否还有从未加载的单元(初始自动加载判断;Error 不算,避免死循环)
    pub fn any_idle(&self, tab: usize) -> bool {
        Self::required_units(tab)
            .iter()
            .any(|k| self.status_of(*k) == CacheStatus::Idle)
    }

    /// 启动目标 tab 缺失(Idle/Error)单元的加载;Loading/Ready 跳过
    pub fn ensure_loaded(&self, tab: usize) {
        for key in Self::required_units(tab) {
            let st = self.status_of(*key);
            if st != CacheStatus::Ready && st != CacheStatus::Loading {
                self.load(*key);
            }
        }
    }

    /// 强制重拉单元(刷新按钮 / CRUD 回写)。加载期间保留旧 data,页面旧数据继续可读。
    pub fn refresh(&self, key: UnitKey) {
        self.load(key);
    }

    fn load(&self, key: UnitKey) {
        if self.in_flight.read().contains(&key) {
            return; // 单飞:同单元并发只拉一次
        }
        let store = *self;
        let mut in_flight = store.in_flight;
        // 立即置 Loading(保留旧 data),UI 即刻感知
        match key {
            UnitKey::Sources => {
                let cur = store.sources.read().clone();
                let mut s = store.sources;
                s.set(CacheState {
                    status: CacheStatus::Loading,
                    data: cur.data,
                    error: String::new(),
                });
            }
            UnitKey::Combineds => {
                let cur = store.combineds.read().clone();
                let mut s = store.combineds;
                s.set(CacheState {
                    status: CacheStatus::Loading,
                    data: cur.data,
                    error: String::new(),
                });
            }
            UnitKey::Preview => {
                let cur = store.preview.read().clone();
                let mut s = store.preview;
                s.set(CacheState {
                    status: CacheStatus::Loading,
                    data: cur.data,
                    error: String::new(),
                });
            }
            UnitKey::Config => {
                let cur = store.config.read().clone();
                let mut s = store.config;
                s.set(CacheState {
                    status: CacheStatus::Loading,
                    data: cur.data,
                    error: String::new(),
                });
            }
        }
        // 失败分支保留旧数据快照：Error 状态 data 置 stale_*（不置 None），
        // 与「刷新期间旧数据保留」spec 一致（四个单元 data 类型不同，须分别捕获）。
        let stale_sources = store.sources.read().data.clone();
        let stale_combineds = store.combineds.read().data.clone();
        let stale_preview = store.preview.read().data.clone();
        let stale_config = store.config.read().data.clone();
        in_flight.write().insert(key);
        spawn(async move {
            let token = store.token.read().clone();
            let mut in_flight = store.in_flight;
            match key {
                UnitKey::Sources => {
                    let next = match fetch_sources(token.as_deref()).await {
                        Ok(d) => CacheState {
                            status: CacheStatus::Ready,
                            data: Some(d),
                            error: String::new(),
                        },
                        Err(e) => CacheState {
                            status: CacheStatus::Error,
                            data: stale_sources,
                            error: e,
                        },
                    };
                    let mut s = store.sources;
                    s.set(next);
                }
                UnitKey::Combineds => {
                    let next = match fetch_combineds(token.as_deref()).await {
                        Ok(d) => CacheState {
                            status: CacheStatus::Ready,
                            data: Some(d),
                            error: String::new(),
                        },
                        Err(e) => CacheState {
                            status: CacheStatus::Error,
                            data: stale_combineds,
                            error: e,
                        },
                    };
                    let mut s = store.combineds;
                    s.set(next);
                }
                UnitKey::Preview => {
                    let next = match fetch_preview(token.as_deref()).await {
                        Ok(d) => CacheState {
                            status: CacheStatus::Ready,
                            data: Some(d),
                            error: String::new(),
                        },
                        Err(e) => CacheState {
                            status: CacheStatus::Error,
                            data: stale_preview,
                            error: e,
                        },
                    };
                    let mut s = store.preview;
                    s.set(next);
                }
                UnitKey::Config => {
                    let next = match fetch_config(token.as_deref()).await {
                        Ok(d) => CacheState {
                            status: CacheStatus::Ready,
                            data: Some(d),
                            error: String::new(),
                        },
                        Err(e) => CacheState {
                            status: CacheStatus::Error,
                            data: stale_config,
                            error: e,
                        },
                    };
                    let mut s = store.config;
                    s.set(next);
                }
            }
            in_flight.write().remove(&key);
        });
    }
}
