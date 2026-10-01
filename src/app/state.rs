//! UI components own their related state rather than sharing flat workspace fields.
use super::*;

pub(super) struct DiffState {
    pub(super) visible: bool,
    pub(super) selected_file: Option<String>,
    pub(super) presentation: DiffPresentation,
    pub(super) rows: Arc<Vec<DiffListRow>>,
    pub(super) list: ListState,
    pub(super) tx: Sender<(u64, DiffLoadResult)>,
    pub(super) rx: Receiver<(u64, DiffLoadResult)>,
    pub(super) request_id: u64,
    pub(super) watch_tx: Sender<u64>,
    pub(super) watch_rx: Receiver<u64>,
    pub(super) watcher_tx: Sender<(u64, notify::Result<notify::RecommendedWatcher>)>,
    pub(super) watcher_rx: Receiver<(u64, notify::Result<notify::RecommendedWatcher>)>,
    pub(super) watch_generation: u64,
    pub(super) watched_project: Option<usize>,
    pub(super) watcher: Option<notify::RecommendedWatcher>,
    pub(super) refresh_due: Option<Instant>,
    pub(super) first_change_at: Option<Instant>,
    pub(super) poll_at: Option<Instant>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) counts: Option<(usize, usize)>,
    pub(super) files: Vec<DiffFile>,
    pub(super) file_stats: Vec<(usize, usize)>,
}

pub(super) struct SyncState {
    pub(super) tx: Sender<SyncUpdate>,
    pub(super) rx: Receiver<SyncUpdate>,
    pub(super) loading: bool,
    pub(super) in_progress: HashSet<(PathBuf, Option<String>)>,
    pub(super) refresh_pending: HashSet<(PathBuf, Option<String>)>,
    pub(super) last_remote_sync: Instant,
    pub(super) last_upstream_fetch: Instant,
}

pub(super) struct PickerState {
    pub(super) folder_search: FolderSearch,
    pub(super) folder_dialog_open: bool,
    pub(super) available_agents: Vec<AgentChoice>,
    pub(super) selection: usize,
    pub(super) input: Entity<InputState>,
}

pub(super) struct SessionRename {
    pub(super) agent_id: u64,
    pub(super) input: Entity<InputState>,
    pub(super) _subscription: Subscription,
}

pub(super) struct ConversationState {
    pub(super) renaming: Option<SessionRename>,
    pub(super) composer: Entity<TextareaState>,
    pub(super) max_rows: usize,
    pub(super) viewport_height: f32,
    pub(super) base_line_height: f32,
    pub(super) file_dialog_open: bool,
    pub(super) chat_list: ListState,
    pub(super) chat_list_agent: Option<u64>,
    pub(super) chat_rows: Vec<ChatRow>,
    pub(super) collapsed_tool_groups: HashSet<(u64, usize)>,
    pub(super) expanded_tool_history: HashSet<(u64, usize)>,
    pub(super) expanded_tool_rows: HashSet<(u64, usize)>,
    pub(super) expanded_thought_rows: HashSet<(u64, usize)>,
    pub(super) prompt_recall: Option<PromptRecall>,
    pub(super) slash_selection: usize,
    pub(super) slash_dismissed: bool,
    pub(super) file_search: Option<FileSearch>,
    pub(super) file_search_project: Option<usize>,
    pub(super) file_scan_tx: Sender<u64>,
    pub(super) file_scan_rx: Receiver<u64>,
    pub(super) file_scan_generation: u64,
    pub(super) file_selection: usize,
    pub(super) file_dismissed: bool,
}

pub(super) struct MobileState {
    pub(super) provider_input: Entity<InputState>,
    pub(super) server: Option<crate::mobile::Server>,
    pub(super) start_tx: Sender<Result<crate::mobile::Server, String>>,
    pub(super) start_rx: Receiver<Result<crate::mobile::Server, String>>,
    pub(super) loading: bool,
    pub(super) endpoint_id: Option<String>,
    pub(super) pairing_visible: bool,
    pub(super) revoke_confirm: Option<String>,
    pub(super) provider_prompt: Option<String>,
    pub(super) qr: Option<Vec<Vec<bool>>>,
    pub(super) last_snapshot: Option<Instant>,
}

impl DiffState {
    pub(super) fn poll_results(&mut self) -> bool {
        let mut changed = false;
        while let Ok((request_id, result)) = self.rx.try_recv() {
            if request_id != self.request_id {
                continue;
            }
            self.loading = false;
            match result {
                Ok(DiffData::Full(files, stats, presentation, selected_file, rows)) => {
                    let scroll_top = self.list.logical_scroll_top();
                    if self
                        .selected_file
                        .as_ref()
                        .is_some_and(|selected| !files.iter().any(|file| &file.path == selected))
                    {
                        self.selected_file = None;
                    }
                    self.rows = if presentation == self.presentation
                        && selected_file == self.selected_file
                    {
                        rows
                    } else {
                        Arc::new(diff_list_rows(
                            &files,
                            &stats,
                            self.selected_file.as_deref(),
                            self.presentation,
                        ))
                    };
                    self.list = ListState::new(self.rows.len(), ListAlignment::Top, px(28.));
                    self.list.scroll_to(scroll_top);
                    self.counts = (!stats.is_empty()).then(|| {
                        stats.iter().copied().fold((0, 0), |total, count| {
                            (total.0 + count.0, total.1 + count.1)
                        })
                    });
                    self.files = files;
                    self.file_stats = stats;
                    self.error = None;
                }
                Ok(DiffData::Counts(counts)) => {
                    self.counts = counts;
                    self.error = None;
                }
                Err(error) => {
                    self.counts = None;
                    self.files.clear();
                    self.file_stats.clear();
                    self.error = Some(error);
                }
            }
            changed = true;
        }
        changed
    }
}

impl SyncState {
    pub(super) fn schedule(&mut self, source_projects: &[ProjectView]) {
        if !self.loading {
            let fetch_upstream = self.last_upstream_fetch.elapsed() >= Duration::from_secs(300);
            if fetch_upstream {
                self.last_upstream_fetch = Instant::now();
            }
            let refresh_remote =
                fetch_upstream || self.last_remote_sync.elapsed() >= Duration::from_secs(30);
            if refresh_remote {
                self.last_remote_sync = Instant::now();
            }
            let mut projects = Vec::new();
            for project in source_projects {
                let path = project.path.clone();
                let host = project.ssh_host.clone();
                let key = (path.clone(), host.clone());
                if self.in_progress.contains(&key)
                    || !(refresh_remote || host.is_none() || self.refresh_pending.contains(&key))
                {
                    continue;
                }
                let forced_fetch = self.refresh_pending.remove(&key);
                projects.push((path, host, fetch_upstream || forced_fetch));
            }
            projects.sort_by_key(|(_, host, _)| host.is_some());
            self.loading = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                for (path, host, fetch) in projects {
                    let counts = host.as_ref().map_or_else(
                        || {
                            if fetch && !crate::git_sync::fetch(&path) {
                                None
                            } else {
                                crate::git_sync::counts(&path)
                            }
                        },
                        |host| crate::git_sync::remote_counts(host, &path, fetch),
                    );
                    if tx.send(SyncUpdate::Counts(path, host, counts)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(SyncUpdate::Finished);
            });
        }
    }
}

impl ConversationState {
    pub(super) fn new(composer: Entity<TextareaState>, window: &Window, font_scale: f32) -> Self {
        let (viewport_height, base_line_height) = Self::composer_geometry(window);
        let (file_scan_tx, file_scan_rx) = mpsc::channel();
        ConversationState {
            renaming: None,
            file_dialog_open: false,
            composer,
            max_rows: Self::composer_max_rows(viewport_height, base_line_height, font_scale),
            viewport_height,
            base_line_height,
            chat_list: ListState::new(0, ListAlignment::Bottom, px(300.)),
            chat_list_agent: None,
            chat_rows: Vec::new(),
            collapsed_tool_groups: HashSet::new(),
            expanded_tool_history: HashSet::new(),
            expanded_tool_rows: HashSet::new(),
            expanded_thought_rows: HashSet::new(),
            prompt_recall: None,
            slash_selection: 0,
            slash_dismissed: false,
            file_search: None,
            file_search_project: None,
            file_scan_tx,
            file_scan_rx,
            file_scan_generation: 0,
            file_selection: 0,
            file_dismissed: false,
        }
    }

    pub(super) fn composer_geometry(window: &Window) -> (f32, f32) {
        (
            f32::from(window.viewport_size().height),
            f32::from(
                window
                    .text_style()
                    .line_height_in_pixels(px(BASE_FONT_SIZE)),
            ),
        )
    }

    pub(super) fn composer_max_rows(
        viewport_height: f32,
        base_line_height: f32,
        font_scale: f32,
    ) -> usize {
        // Keep room for the header, controls, and some conversation above the draft.
        let available_height = viewport_height - 200.;
        (available_height / (base_line_height * font_scale))
            .floor()
            .max(1.) as usize
    }

    pub(super) fn new_composer(
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<TextareaState> {
        let font_scale = f32::from(Theme::global(cx).font_size) / BASE_FONT_SIZE;
        let (viewport_height, base_line_height) = Self::composer_geometry(window);
        cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(
                    1,
                    Self::composer_max_rows(viewport_height, base_line_height, font_scale),
                )
                .submit_on_enter(true)
                .placeholder("Ask your agent…")
        })
    }

    pub(super) fn resize_composers(&mut self, font_scale: f32, cx: &mut Context<Workspace>) {
        let max_rows =
            Self::composer_max_rows(self.viewport_height, self.base_line_height, font_scale);
        if max_rows == self.max_rows {
            return;
        }
        self.max_rows = max_rows;
        self.composer
            .update(cx, |input, cx| input.set_auto_grow(1, max_rows, cx));
    }
}

impl PickerState {
    pub(super) fn open(
        &mut self,
        step: PickerStep,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        self.selection = 0;
        if matches!(step, PickerStep::Folders | PickerStep::ChangeFolder { .. }) {
            self.folder_search.set_query("");
        }
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(
                match step {
                    PickerStep::Agents { .. } => "Search installed agents or enter an ACP command…",
                    PickerStep::Folders | PickerStep::ChangeFolder { .. } => {
                        "Search recent folders or enter a local or SSH path…"
                    }
                },
                window,
                cx,
            );
            input.focus(window, cx);
        });
    }
}

impl MobileState {
    pub(super) fn link(&self) -> Option<String> {
        let endpoint_id = self.endpoint_id.as_ref()?;
        let token = self.server.as_ref()?.pairing_token.lock().ok()?.clone();
        let base =
            std::env::var("AGENTAPS_WEB_URL").unwrap_or_else(|_| "https://agentaps.dev/".into());
        Some(format!(
            "{}#{}:{token}",
            base.trim_end_matches('#'),
            endpoint_id
        ))
    }
}
