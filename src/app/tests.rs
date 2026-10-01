use super::*;
use crate::session::test_agent;

#[test]
fn diff_file_expands_beneath_its_row_without_hiding_other_files() {
    use crate::diff_view::{Hunk, Line, Mark, Side};

    let files = [
        DiffFile {
            path: "first.txt".into(),
            hunks: vec![Hunk {
                header: "@@ -0,0 +1 @@".into(),
                lines: vec![Line {
                    old: None,
                    new: Some(Side {
                        number: 1,
                        text: "added line".into(),
                        mark: Mark::Added,
                    }),
                }],
            }],
            note: None,
        },
        DiffFile {
            path: "second.txt".into(),
            hunks: Vec::new(),
            note: Some("Second file note".into()),
        },
    ];
    let stats = [(1, 0), (0, 0)];
    let rows = diff_list_rows(&files, &stats, Some("first.txt"), DiffPresentation::Unified);
    assert!(
        matches!(&rows[0], DiffListRow::File { path, expanded: true, .. } if path == "first.txt")
    );
    assert!(
        matches!(&rows[1], DiffListRow::Content(DiffRow::Hunk(header)) if header == "@@ -0,0 +1 @@")
    );
    assert!(
        matches!(&rows[2], DiffListRow::Content(DiffRow::Unified { side, .. }) if side.text == "added line")
    );
    assert!(
        matches!(&rows[3], DiffListRow::File { path, expanded: false, .. } if path == "second.txt")
    );
    assert_eq!(rows.len(), 4);

    let collapsed = diff_list_rows(&files, &stats, None, DiffPresentation::Unified);
    assert_eq!(collapsed.len(), 2);
}

#[test]
fn branch_labels_unborn_and_detached_heads_without_git() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("agentaps-branch-{}-{stamp}", std::process::id()));
    let repo = gix::init(&path).unwrap();
    std::fs::write(repo.git_dir().join("HEAD"), "ref: refs/heads/topic\n").unwrap();
    assert_eq!(branch(&path), "topic");

    let id = repo.write_blob(b"detached head fixture").unwrap();
    std::fs::write(repo.git_dir().join("HEAD"), format!("{id}\n")).unwrap();
    let detached = branch(&path);
    assert!(id.to_string().starts_with(&detached));
    assert!(detached.len() < id.to_string().len());

    drop(repo);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn session_search_prefers_direct_name_matches() {
    let direct = session_search_score(
        "agentaps",
        Path::new("/dev/agentaps"),
        "main",
        "Codex",
        None,
    );
    let path_only = session_search_score(
        "agentaps",
        Path::new("/dev/agentaps/examples"),
        "main",
        "Codex",
        None,
    );
    assert!(direct > path_only);
    assert!(
        session_search_score("codex", Path::new("/dev/project"), "main", "Codex", None).is_some()
    );
    assert!(
        session_search_score(
            "login",
            Path::new("/dev/project"),
            "main",
            "Codex",
            Some("Fix login flow")
        )
        .is_some()
    );
    assert_eq!(
        session_search_score("missing", Path::new("/dev/project"), "main", "Codex", None),
        None
    );
}

#[test]
fn workspace_view_keeps_sidebar_selection_exclusive() {
    let first = SessionLocation {
        project_index: 0,
        agent_index: 0,
    };
    let second = SessionLocation {
        project_index: 0,
        agent_index: 1,
    };
    let conversation = WorkspaceView::Conversation(first);
    assert_eq!(conversation.highlighted_session(), Some(first));
    let archive = conversation.toggle_archive();
    assert_eq!(archive.highlighted_session(), None);
    assert_eq!(archive.displayed_session(), Some(first));
    assert_eq!(archive.toggle_archive(), conversation);

    let folders = conversation.open_picker(PickerStep::Folders);
    assert_eq!(folders.highlighted_session(), None);
    assert_eq!(folders.displayed_session(), None);
    assert_eq!(folders.return_to(), Some(first));

    let changing_folder = conversation.open_picker(PickerStep::ChangeFolder { session: first });
    assert_eq!(changing_folder.return_to(), Some(first));
    assert_eq!(changing_folder.displayed_session(), None);
    assert_eq!(
        changing_folder.session_archived(first, Some(second)),
        WorkspaceView::NewSession {
            step: PickerStep::Folders,
            return_to: Some(second),
        }
    );

    let agents = folders.open_picker(PickerStep::Agents { project_index: 0 });
    assert_eq!(agents.highlighted_session(), None);
    assert_eq!(agents.return_to(), Some(first));

    assert_eq!(agents.toggle_archive(), archive);

    let updated = agents.session_archived(first, Some(second));
    assert_eq!(updated.highlighted_session(), None);
    assert_eq!(updated.return_to(), Some(second));
    assert_eq!(
        updated.toggle_archive().toggle_archive(),
        WorkspaceView::Conversation(second)
    );
    assert_eq!(
        conversation.session_archived(first, None),
        WorkspaceView::Empty
    );
}

#[test]
fn sidebar_icons_are_bundled() {
    assert!(
        gpui_kit::AssetSource::load(&AppAssets, "icons/inbox.svg")
            .unwrap()
            .is_some()
    );
    assert!(
        gpui_kit::AssetSource::load(&AppAssets, "icons/undo-2.svg")
            .unwrap()
            .is_some()
    );
    assert!(
        gpui_kit::AssetSource::load(&AppAssets, "icons/mobile.svg")
            .unwrap()
            .is_some()
    );
}

#[test]
fn prompt_recall_walks_history_and_restores_draft() {
    let history = vec!["first".into(), "second\nline".into()];
    let mut recall = None;
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "draft", false),
        None
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "draft", true),
        Some("second\nline".into())
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "second\nline", true),
        Some("first".into())
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "first", true),
        Some("first".into())
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "first", false),
        Some("second\nline".into())
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 7, &history, "second\nline", false),
        Some("draft".into())
    );
    assert_eq!(
        PromptRecall::step(&mut recall, 8, &history, "other draft", true),
        Some("second\nline".into())
    );
    assert_eq!(recall.unwrap().draft, "other draft");
}

#[test]
fn sidebar_drag_moves_in_both_directions() {
    let mut order = vec![1, 2, 3, 4];
    assert!(move_sidebar_id(&mut order, 1, 3));
    assert_eq!(order, vec![2, 3, 1, 4]);
    assert!(move_sidebar_id(&mut order, 4, 2));
    assert_eq!(order, vec![4, 2, 3, 1]);
    assert!(!move_sidebar_id(&mut order, 4, 4));
}

#[test]
fn adjacent_tool_calls_form_one_group() {
    let mut agent = test_agent(ProtocolVersion::V2);
    agent.log(Role::Tool, "first tool");
    agent.log(Role::Tool, "second tool");
    agent.log(Role::Agent, "answer");
    agent.log(Role::Tool, "later tool");
    assert_eq!(tool_run_end(&agent.messages, 0), 2);
    assert_eq!(tool_run_end(&agent.messages, 3), 4);
    agent.config.archived = true;
    assert!(agent.snapshot().archived);
}

#[test]
fn tool_activity_describes_search_read_and_review() {
    assert_eq!(
        tool_description("rg -n 'guardian|review|tool call' src"),
        ("Search guardian|review|tool call in src".into(), true)
    );
    assert_eq!(
        tool_description("/bin/bash -lc \"sed -n '1,80p' src/main.rs\""),
        ("Read src/main.rs".into(), true)
    );
    assert_eq!(
        tool_description("cd /project && head -80 README.md && ls && git log --oneline | wc -l"),
        ("Inspect project (4 steps)".into(), true)
    );
    assert_eq!(
        tool_description("cat src/app.rs && cat src/app/tests.rs && cat src/app/render.rs"),
        ("Read files (3 steps)".into(), true)
    );
    assert_eq!(
        tool_description("cargo fmt --all -- --check && git diff --check"),
        ("Check formatting · Check patch whitespace".into(), true)
    );
    assert_eq!(
        tool_description("cargo test --locked file_mention_completes_at_cursor"),
        ("Run targeted test".into(), true)
    );
    let mut agent = test_agent(ProtocolVersion::V1);
    agent.log(Role::Tool, "rg -n guardian src");
    agent.log(Role::Tool, "Guardian Review");
    agent.log(Role::Tool, "cat README.md");
    assert_eq!(tool_group_heading(&agent.messages), "Activity");
    assert_eq!(markdown_code_block("a ``` b"), "````\na ``` b\n````");
}

#[test]
fn tool_group_heading_tracks_live_and_finished_steps() {
    let mut agent = test_agent(ProtocolVersion::V2);
    agent.log(Role::Tool, "cat src/app.rs · completed");
    agent.log(Role::Tool, "cargo test --locked cursor_test · in_progress");
    agent.log(Role::Tool, "Guardian Review · completed");
    assert_eq!(
        tool_group_heading(&agent.messages),
        "Working · Running targeted test"
    );
    agent.messages[1].text = "cargo test --locked cursor_test · completed".into();
    assert_eq!(tool_group_heading(&agent.messages), "Completed");
    agent.log(Role::Tool, "Using tool · in_progress");
    assert_eq!(tool_group_heading(&agent.messages), "Working · Using tool");
    agent.messages.pop();
    agent.messages[2].text = "Guardian Review · pending".into();
    assert_eq!(tool_group_heading(&agent.messages), "Completed");
    agent.messages[2].text = "Guardian Review · failed".into();
    assert_eq!(tool_group_heading(&agent.messages), "Completed");
}

#[test]
fn file_mention_completes_at_cursor_without_changing_surrounding_text() {
    let draft = "Check 🦀 @src/ma and @other";
    let cursor = Position::new(0, "Check 🦀 @src/ma".encode_utf16().count() as u32);
    let (range, query) = file_mention(draft, cursor).unwrap();
    assert_eq!(query, "src/ma");
    let (completed, position) = completed_file_text(draft, range, "src/main.rs");
    assert_eq!(completed, "Check 🦀 @src/main.rs and @other");
    assert_eq!(
        position.character,
        "Check 🦀 @src/main.rs ".encode_utf16().count() as u32
    );
    assert!(file_mention(&completed, position).is_none());
    let draft = "Review @src/main soon";
    let cursor = Position::new(0, "Review @src/ma".encode_utf16().count() as u32);
    let (range, query) = file_mention(draft, cursor).unwrap();
    assert_eq!(query, "src/ma");
    let (completed, position) = completed_file_text(draft, range, "src/main.rs");
    assert_eq!(completed, "Review @src/main.rs soon");
    assert!(file_mention(&completed, position).is_none());
    let draft = "Review @src/ma";
    let cursor = Position::new(0, draft.encode_utf16().count() as u32);
    let (range, _) = file_mention(draft, cursor).unwrap();
    let (completed, position) = completed_file_text(draft, range, "src/main.rs");
    assert_eq!(completed, "Review @src/main.rs ");
    assert!(file_mention(&completed, position).is_none());
    assert!(file_mention("email@example.com", Position::new(0, 17)).is_none());
}

#[test]
fn slash_commands_follow_agent_snapshots_and_complete_with_input_hint() {
    let mut agent = test_agent(ProtocolVersion::V1);
    SessionController::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"review","description":"Review changes","input":{"hint":"files"}},
                {"name":"reset","description":"Reset chat"}
            ]
        }}}),
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/re").len(),
        2
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/review ").len(),
        0
    );
    assert_eq!(
        completed_slash_text(&agent.config.available_commands[0]),
        "/review "
    );
    assert_eq!(
        completed_slash_text(&agent.config.available_commands[1]),
        "/reset"
    );

    SessionController::handle_update(
        &mut agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"plan","description":"Plan work"}
            ]
        }}}),
    );
    assert_eq!(
        matching_slash_commands(&agent.config.available_commands, "/re").len(),
        0
    );

    let mut v2_agent = self::test_agent(ProtocolVersion::V2);
    SessionController::handle_update(
        &mut v2_agent,
        &json!({"params":{"sessionId":"session-1","update":{
            "sessionUpdate":"available_commands_update","availableCommands":[
                {"name":"plan","description":"Plan work"}
            ]
        }}}),
    );
    assert_eq!(v2_agent.config.available_commands[0].name, "plan");
}

#[test]
fn submitting_keeps_explicit_newlines() {
    assert_eq!(submitted_prompt("first\nsecond\n"), "first\nsecond");
    assert_eq!(submitted_prompt("first\n\n"), "first\n");
}

#[test]
fn shell_mode_requires_first_character_and_preserves_command_text() {
    assert_eq!(
        shell_command("!  printf 'hello'\n"),
        Some("  printf 'hello'\n")
    );
    assert_eq!(shell_command(" !printf hello"), None);
    assert_eq!(shell_command("! \n"), None);
    assert_eq!(
        shell_command_in_message("!printf hello"),
        Some("printf hello")
    );
    assert_eq!(
        shell_command_in_message(&prompt_for_agent("!printf hello")),
        Some("printf hello")
    );
    assert_eq!(prompt_for_agent("ask ! for help"), "ask ! for help");
    assert_eq!(
        prompt_for_agent("!  printf 'hello'\nnext"),
        "Run this shell command exactly as written, then report its output:\n\n  printf 'hello'\nnext"
    );
}

#[gpui_kit::test]
fn zoom_shortcuts_and_menu_actions_change_the_font_scale(cx: &mut gpui_kit::TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    config::save_to(
        &temp.path().join("agentaps/config.json"),
        &Config::default(),
    )
    .unwrap();
    let config_env = if cfg!(windows) {
        "APPDATA"
    } else {
        "XDG_CONFIG_HOME"
    };
    let original_config_home = std::env::var_os(config_env);
    // SAFETY: no other test in this process reads the config directory while this
    // test runs; Workspace::new is only constructed here.
    unsafe { std::env::set_var(config_env, temp.path()) };
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::apply(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-=", ZoomIn, None),
            KeyBinding::new("cmd--", ZoomOut, None),
            KeyBinding::new("cmd-0", ZoomReset, None),
        ]);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    // The zoom must land on the theme: in the real app the gpui-component
    // Root plugin resets window.rem_size to theme.font_size before every
    // frame, so theme.font_size is what actually reaches the paint.
    let zoom_state = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            let theme = Theme::global(cx);
            (
                workspace.read(cx).font_scale,
                theme.font_size,
                theme.mono_font_size,
            )
        })
    };

    assert_eq!(
        zoom_state(cx),
        (1.0, px(BASE_FONT_SIZE), px(BASE_MONO_FONT_SIZE))
    );
    let default_max_rows = cx.update(|_, cx| workspace.read(cx).conversation.max_rows);
    cx.simulate_keystrokes("cmd-=");
    assert_eq!(
        zoom_state(cx),
        (1.1, px(BASE_FONT_SIZE * 1.1), px(BASE_MONO_FONT_SIZE * 1.1)),
        "Cmd+= should zoom in"
    );
    cx.update(|_, cx| workspace.update(cx, |this, _| this.persistence.wait().unwrap()));
    let (saved, _) = config::load().expect("zooming should save the config");
    assert_eq!(saved.font_scale, 1.1, "Cmd+= should persist the zoom level");
    assert!(
        cx.update(|_, cx| workspace.read(cx).conversation.max_rows) < default_max_rows,
        "zooming in should reserve more room above a long draft"
    );
    cx.simulate_keystrokes("cmd--");
    assert_eq!(
        zoom_state(cx),
        (1.0, px(BASE_FONT_SIZE), px(BASE_MONO_FONT_SIZE)),
        "Cmd+- should zoom out"
    );
    cx.dispatch_action(ZoomIn);
    assert_eq!(
        zoom_state(cx),
        (1.1, px(BASE_FONT_SIZE * 1.1), px(BASE_MONO_FONT_SIZE * 1.1)),
        "menu-dispatched ZoomIn should zoom in"
    );
    cx.dispatch_action(ZoomReset);
    assert_eq!(
        zoom_state(cx),
        (1.0, px(BASE_FONT_SIZE), px(BASE_MONO_FONT_SIZE)),
        "menu-dispatched ZoomReset should reset"
    );
    cx.update(|_, cx| workspace.update(cx, |this, _| this.persistence.wait().unwrap()));
    let (saved, _) = config::load().expect("zooming should save the config");
    assert_eq!(saved.font_scale, 1.0, "ZoomReset should persist the reset");

    // The acknowledgement notice hides itself after ZOOM_NOTICE_TIMEOUT.
    let notice = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| {
            workspace
                .read(cx)
                .notice
                .as_ref()
                .map(|notice| notice.message().to_owned())
        })
    };
    cx.dispatch_action(ZoomIn);
    cx.dispatch_action(ZoomIn);
    assert!(cx.update(|_, cx| matches!(workspace.read(cx).notice, Some(Notice::Info(_)))));
    assert_eq!(notice(cx), Some("Zoom 120%".to_string()));
    cx.executor().advance_clock(ZOOM_NOTICE_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(
        notice(cx),
        None,
        "zoom notice should hide after the timeout"
    );

    // A later error or unrelated notice must survive the zoom timer.
    cx.dispatch_action(ZoomIn);
    cx.update(|_, cx| {
        workspace.update(cx, |this, _| {
            this.notice = Some(Notice::Error("Another notice".into()))
        })
    });
    cx.executor().advance_clock(ZOOM_NOTICE_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(notice(cx), Some("Another notice".to_string()));

    // Git results carry their severity into the banner and can be dismissed.
    for (action, result, expected) in [
        (
            crate::git_sync::SyncAction::Pull,
            Ok((0, 0)),
            Notice::Info("Commits pulled.".into()),
        ),
        (
            crate::git_sync::SyncAction::Push,
            Ok((0, 0)),
            Notice::Info("Commits pushed.".into()),
        ),
        (
            crate::git_sync::SyncAction::Pull,
            Err("connection lost".into()),
            Notice::Error("Could not sync commits: connection lost".into()),
        ),
    ] {
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.sync
                    .tx
                    .send(SyncUpdate::OperationFinished {
                        path: temp.path().to_owned(),
                        host: None,
                        action,
                        result,
                        counts: None,
                    })
                    .unwrap();
                this.poll_sync_counts(cx);
                assert_eq!(this.notice, Some(expected));
                this.dismiss_notice(cx);
                assert_eq!(this.notice, None);
            });
        });
    }

    // Theme changes must retain zoom, save the selection, and restore it on launch.
    cx.update(|_, cx| {
        workspace.update(cx, |this, cx| {
            this.select_theme(crate::appearance::Choice::SolarizedLight, cx);
            assert_eq!(
                Theme::global(cx).mode,
                gpui_kit::component::theme::ThemeMode::Light
            );
            assert_eq!(Theme::global(cx).font_size, px(BASE_FONT_SIZE * 1.3));
            assert_eq!(theme::palette(cx).color(BG), Theme::global(cx).background);
        })
    });
    cx.update(|_, cx| workspace.update(cx, |this, _| this.persistence.wait().unwrap()));
    assert_eq!(
        config::load().unwrap().0.theme,
        crate::appearance::Choice::SolarizedLight
    );

    // Finish the previous snapshot before replacing the writer with a failing one.
    cx.update(|_, cx| workspace.update(cx, |this, _| this.persistence.wait().unwrap()));
    let blocked = temp.path().join("blocked");
    std::fs::write(&blocked, "").unwrap();
    cx.update(|_, cx| {
        workspace.update(cx, |this, _| {
            this.persistence =
                crate::persistence::Persistence::at_path(false, Ok(blocked.join("config.json")));
        })
    });
    cx.dispatch_action(ZoomIn);
    cx.update(|_, cx| {
        workspace.update(cx, |this, _| {
            let error = this.persistence.wait().unwrap_err();
            this.notice = Some(Notice::Error(format!("Could not save config: {error}")));
        })
    });
    assert!(notice(cx).unwrap().starts_with("Could not save config:"));
    cx.executor().advance_clock(ZOOM_NOTICE_TIMEOUT);
    cx.run_until_parked();
    assert!(notice(cx).unwrap().starts_with("Could not save config:"));

    // Restore the saved zoom in a fresh workspace after resetting the theme.
    // SAFETY: this test is the only test constructing Workspace in this process.
    unsafe { std::env::set_var(config_env, temp.path()) };
    cx.update(|_, cx| theme::apply(cx));
    let (restored, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_, cx| {
        let restored = restored.read(cx);
        assert_eq!(restored.font_scale, 1.3);
        assert_eq!(
            restored.theme_choice,
            crate::appearance::Choice::SolarizedLight
        );
        assert_eq!(
            Theme::global(cx).mode,
            gpui_kit::component::theme::ThemeMode::Light
        );
        assert_eq!(Theme::global(cx).font_size, px(BASE_FONT_SIZE * 1.3));
        assert_eq!(
            Theme::global(cx).mono_font_size,
            px(BASE_MONO_FONT_SIZE * 1.3)
        );
        assert!(
            restored.conversation.max_rows < default_max_rows,
            "restoring zoom should also limit the composer height"
        );
    });

    // No-op reset must not schedule a timer that removes another notice.
    cx.dispatch_action(ZoomReset);
    cx.executor().advance_clock(ZOOM_NOTICE_TIMEOUT);
    cx.run_until_parked();
    cx.update(|_, cx| {
        restored.update(cx, |this, _| {
            this.notice = Some(Notice::Error("Keep this notice".into()))
        })
    });
    cx.dispatch_action(ZoomReset);
    cx.executor().advance_clock(ZOOM_NOTICE_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| restored
            .read(cx)
            .notice
            .as_ref()
            .map(|notice| notice.message().to_owned())),
        Some("Keep this notice".to_string())
    );
    cx.update(|_, cx| restored.update(cx, |this, _| this.persistence.wait().unwrap()));
    mobile::verify_session_switching_and_mobile_routing(&restored, temp.path(), cx);
    verify_resets_and_folder_moves_preserve_harness_references(&restored, temp.path(), cx);
    verify_inline_session_renaming(&restored, cx);
    verify_split_panes(&restored, temp.path(), cx);
    // SAFETY: restore the process environment modified for this test.
    unsafe {
        if let Some(original) = original_config_home {
            std::env::set_var(config_env, original);
        } else {
            std::env::remove_var(config_env);
        }
    }
}

#[test]
fn v1_tool_updates_replace_the_same_call_and_keep_approval_status() {
    let mut review_agent = test_agent(ProtocolVersion::V1);
    for update in [
        json!({"sessionUpdate":"tool_call","toolCallId":"guardian-1",
            "title":"Guardian Review","status":"in_progress"}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"guardian-1",
            "status":"completed"}),
    ] {
        SessionController::handle_update(
            &mut review_agent,
            &json!({"params":{"sessionId":"session-1","update":update}}),
        );
    }
    assert_eq!(review_agent.messages.len(), 1);
    assert_eq!(review_agent.messages[0].text, "Guardian Review · completed");
    assert_eq!(tool_group_heading(&review_agent.messages), "Activity");
    let mut tool = test_agent(ProtocolVersion::V2);
    tool.upsert_tool_call(
        &json!({"toolCallId":"command-1","title":"cat README.md","status":"in_progress"}),
    );
    tool.messages[0].text.push_str("\noutput text");
    tool.upsert_tool_call(&json!({"toolCallId":"command-1","status":"completed"}));
    assert_eq!(
        tool.messages[0].text,
        "cat README.md · completed\noutput text"
    );
}

fn verify_resets_and_folder_moves_preserve_harness_references(
    workspace: &Entity<Workspace>,
    path: &Path,
    cx: &mut gpui_kit::VisualTestContext,
) {
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| {
            let mut original = test_agent(ProtocolVersion::V2);
            original.config.command.clear(); // No harness process is needed for this UI state check.
            original.active_work = false;
            original.log(Role::User, "Previous conversation");
            this.projects = vec![ProjectView {
                path: path.to_owned(),
                ssh_host: None,
                branch: "main".into(),
                sync_counts: None,
                agents: vec![AgentView {
                    controller: original,
                    elicitations: Vec::new(),
                }],
            }];
            this.next_agent_id = 200;
            this.sidebar_order = vec![1];
            this.deferred_connections.clear();
            this.reset_context(0, 0, cx);
            assert_eq!(this.projects[0].agents.len(), 2);
            let archived = &this.projects[0].agents[1];
            assert!(archived.config.archived);
            assert_eq!(archived.config.session_id.as_deref(), Some("session-1"));
            assert!(archived.messages.is_empty());
            assert!(
                this.projects[0].agents[0]
                    .messages
                    .iter()
                    .all(|entry| entry.role != Role::User)
            );
            this.projects[0].agents[0].controller.session_id = Some("reset-session".into());
            this.projects[0].agents[0].config.session_has_activity = true;
            this.projects.push(ProjectView {
                path: path.join("other"),
                ssh_host: None,
                branch: "main".into(),
                sync_counts: None,
                agents: Vec::new(),
            });
            this.move_session_to_project(
                SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                },
                1,
                window,
                cx,
            );
            assert!(this.projects[0].agents[0].config.archived);
            assert_eq!(
                this.projects[0].agents[0].config.session_id.as_deref(),
                Some("reset-session")
            );
            assert_eq!(this.projects[1].agents.len(), 1);
            assert!(!this.projects[1].agents[0].config.archived);
            this.persistence.wait().unwrap();
            let (saved, _) = config::load().unwrap();
            assert_eq!(
                saved.projects[0].agents[0].session_id.as_deref(),
                Some("reset-session")
            );
            assert_eq!(
                saved.projects[0].agents[1].session_id.as_deref(),
                Some("session-1")
            );
            assert!(
                saved
                    .projects
                    .iter()
                    .flat_map(|project| &project.agents)
                    .all(|agent| agent.messages.is_empty())
            );
            this.projects.clear();
            this.view = WorkspaceView::Empty;
            this.persistence.dirty = false;
        })
    });
}

fn verify_inline_session_renaming(
    workspace: &Entity<Workspace>,
    cx: &mut gpui_kit::VisualTestContext,
) {
    let agent_id = 301;
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| {
            let mut agent = test_agent(ProtocolVersion::V2);
            agent.config.id = agent_id;
            agent.config.command.clear();
            this.projects = vec![ProjectView {
                path: PathBuf::from("."),
                ssh_host: None,
                branch: String::new(),
                sync_counts: None,
                agents: vec![AgentView {
                    controller: agent,
                    elicitations: Vec::new(),
                }],
            }];
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                }),
                window,
                cx,
            );
        });
    });
    for (draft, save) in [("Inline name", true), ("Discard this", false)] {
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.open_rename_session(agent_id, window, cx);
                let input = this.conversation.renaming.as_ref().unwrap().input.clone();
                input.update(cx, |input, cx| input.set_value(draft, window, cx));
            });
        });
        cx.run_until_parked();
        if save {
            cx.dispatch_action(Enter {
                secondary: false,
                shift: false,
            });
        } else {
            cx.dispatch_action(Escape);
        }
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |this, _| {
                assert!(
                    this.conversation.renaming.is_none(),
                    "editor remained open: save={save}"
                );
                assert_eq!(
                    this.agent_mut(agent_id).unwrap().0.config.session_title(),
                    Some("Inline name")
                );
            });
        });
    }
}

fn verify_split_panes(
    workspace: &Entity<Workspace>,
    path: &Path,
    cx: &mut gpui_kit::VisualTestContext,
) {
    use crate::panes::{Direction, Layout, Node};
    let first_file = path.join("first-pane.txt");
    let second_file = path.join("second-pane.txt");
    std::fs::write(&first_file, "First pane attachment").unwrap();
    std::fs::write(&second_file, "Second pane attachment").unwrap();
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| {
            let mut agents = Vec::new();
            for id in [701, 702] {
                let mut agent = test_agent(ProtocolVersion::V2);
                agent.config.id = id;
                agent.status = Status::Connecting;
                agent.active_work = false;
                for index in 0..40 {
                    agent.log(Role::User, format!("Session {id}, message {index}"));
                }
                agents.push(AgentView {
                    controller: agent,
                    elicitations: Vec::new(),
                });
            }
            this.projects = vec![ProjectView {
                path: path.to_owned(),
                ssh_host: None,
                branch: "main".into(),
                sync_counts: None,
                agents,
            }];
            this.sidebar_order = vec![701, 702];
            this.deferred_connections.clear();
            this.pane_layout = Layout::default();
            this.pane_id = 1;
            this.next_pane_id = 2;
            this.inactive_panes.clear();
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                }),
                window,
                cx,
            );
            this.pane_bounds.insert(
                1,
                std::rc::Rc::new(std::cell::Cell::new(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(800.)),
                ))),
            );
            this.split_pane(Direction::Right, window, cx);
            assert_eq!(this.pane_id, 3);
            assert_eq!(this.view, WorkspaceView::Empty);
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 1,
                }),
                window,
                cx,
            );
            this.sync_chat_rows(0, 1);
            this.conversation.chat_list.scroll_to(gpui_kit::ListOffset {
                item_ix: 5,
                offset_in_item: px(0.),
            });
            this.conversation.composer.update(cx, |input, cx| {
                input.set_value("second pane prompt", window, cx);
                input.focus(window, cx);
            });
            this.with_pane(1, cx, |this, cx| {
                this.drop_paths(
                    &gpui_kit::ExternalPaths([first_file.clone()].into_iter().collect()),
                    cx,
                );
                this.sync_chat_rows(0, 0);
                this.conversation.chat_list.scroll_to(gpui_kit::ListOffset {
                    item_ix: 12,
                    offset_in_item: px(0.),
                });
                this.conversation.composer.update(cx, |input, cx| {
                    input.set_value("first pane draft", window, cx)
                });
            });
            assert_eq!(
                this.pane_id, 3,
                "rendering another pane must preserve focus"
            );
            this.choose_attachments(window, cx);
        });
    });
    cx.run_until_parked();
    // The composer focused by keyboard remains the routing source even if the
    // sidebar/pane selection changes before its input event is delivered.
    cx.update(|_, cx| {
        workspace.update(cx, |this, cx| {
            this.activate_pane(1, cx);
        });
    });
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.multiple && !options.directories);
        Some(vec![second_file.clone()])
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |this, _| {
            assert_eq!(
                this.pane_layout.focused, 1,
                "the chooser must not steal pane focus"
            );
            assert_eq!(this.draft_files[&701][0].name, "first-pane.txt");
            assert_eq!(this.draft_files[&702][0].name, "second-pane.txt");
        })
    });
    cx.dispatch_action(Enter {
        secondary: false,
        shift: false,
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| {
            assert_eq!(this.pane_layout.focused, 3);
            assert_eq!(
                this.projects[0].agents[1].config.pending_prompts[0].text,
                "second pane prompt"
            );
            assert_eq!(
                this.projects[0].agents[1].config.pending_prompts[0].files[0].name,
                "second-pane.txt"
            );
            assert!(this.projects[0].agents[0].config.pending_prompts.is_empty());
            assert_eq!(this.conversation.chat_list.logical_scroll_top().item_ix, 5);
            this.with_pane(1, cx, |this, cx| {
                assert_eq!(
                    this.conversation.composer.read(cx).value().as_ref(),
                    "first pane draft"
                );
                assert_eq!(this.conversation.chat_list.logical_scroll_top().item_ix, 12);
            });
            // Sidebar selection focuses the existing pane instead of displaying
            // the same session in two panes.
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                }),
                window,
                cx,
            );
            assert_eq!(this.pane_id, 1);
            assert_eq!(
                this.saved_pane_layout().root.panes(),
                [(1, Some(701)), (3, Some(702))]
            );
            this.diff.visible = true;
            this.diff.counts = Some((99, 99));
            let request = this.diff.request_id;
            this.activate_pane(3, cx);
            assert!(this.diff.visible);
            assert_ne!(this.diff.request_id, request);
            assert_eq!(this.diff.counts, None);
            this.persist();
            this.persistence.wait().unwrap();
        });
    });
    let saved = config::load().unwrap().0.pane_layout.unwrap();
    assert_eq!(saved.root.panes(), [(1, Some(701)), (3, Some(702))]);
    assert_eq!(saved.focused, 3);
    // Exercise pane-bound callbacks after closing their source pane.
    let callback = cx.update(|_, cx| {
        workspace.update(cx, |this, cx| {
            this.pane_listener(cx, |this, _: &(), window, cx| this.send_prompt(window, cx))
        })
    });
    cx.update(|window, cx| {
        workspace.update(cx, |this, cx| {
            this.close_pane(window, cx);
            assert_eq!(this.pane_id, 1);
            assert!(matches!(this.pane_layout.root, Node::Pane { id: 1, .. }));
            assert!(!this.projects[0].agents[1].config.archived);
        });
        callback(&(), window, cx);
        workspace.update(cx, |this, cx| {
            assert_eq!(
                this.conversation.composer.read(cx).value().as_ref(),
                "first pane draft"
            );
            assert!(this.projects[0].agents[0].config.pending_prompts.is_empty());
            // A hidden session's draft follows it to another pane, and survives
            // closing that pane. Drafts belong to sessions, not presentation slots.
            let mut third = test_agent(ProtocolVersion::V2);
            third.config.id = 703;
            third.status = Status::Connecting;
            third.active_work = false;
            this.projects[0].agents.push(AgentView {
                controller: third,
                elicitations: Vec::new(),
            });
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 2,
                }),
                window,
                cx,
            );
            this.pane_bounds.insert(
                1,
                std::rc::Rc::new(std::cell::Cell::new(Bounds::new(
                    Default::default(),
                    size(px(800.), px(800.)),
                ))),
            );
            this.split_pane(Direction::Down, window, cx);
            let moved_pane = this.pane_id;
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                }),
                window,
                cx,
            );
            assert_eq!(this.pane_id, moved_pane);
            assert_eq!(this.draft_files[&701][0].name, "first-pane.txt");
            assert_eq!(
                this.conversation.composer.read(cx).value().as_ref(),
                "first pane draft"
            );
            this.close_pane(window, cx);
            this.set_view(
                WorkspaceView::Conversation(SessionLocation {
                    project_index: 0,
                    agent_index: 0,
                }),
                window,
                cx,
            );
            assert_eq!(
                this.conversation.composer.read(cx).value().as_ref(),
                "first pane draft"
            );
            let first = SessionLocation {
                project_index: 0,
                agent_index: 0,
            };
            let second = SessionLocation {
                project_index: 0,
                agent_index: 1,
            };
            // Sessions in the same checkout share the loaded diff, including
            // its rows and selected file, without needing a filesystem event.
            this.diff.visible = true;
            this.diff.loading = false;
            this.diff.files = vec![DiffFile {
                path: "changed.txt".into(),
                hunks: Vec::new(),
                note: None,
            }];
            this.diff.file_stats = vec![(1, 0)];
            this.diff.selected_file = Some("changed.txt".into());
            this.diff.rows = Arc::new(diff_list_rows(
                &this.diff.files,
                &this.diff.file_stats,
                this.diff.selected_file.as_deref(),
                this.diff.presentation,
            ));
            this.diff.list.reset(this.diff.rows.len());
            let rows = this.diff.rows.clone();
            let request = this.diff.request_id;
            this.set_view(WorkspaceView::Conversation(second), window, cx);
            assert!(this.diff.visible);
            assert!(Arc::ptr_eq(&this.diff.rows, &rows));
            assert_eq!(this.diff.request_id, request);
            assert_eq!(this.diff.selected_file.as_deref(), Some("changed.txt"));
            this.set_view(WorkspaceView::Conversation(first), window, cx);

            // A picker reserves its previous session. Selecting that session
            // from another pane must reveal it in its original pane.
            this.open_picker(PickerStep::Folders, window, cx);
            this.split_pane(Direction::Right, window, cx);
            let other_pane = this.pane_id;
            this.set_view(WorkspaceView::Conversation(second), window, cx);
            this.set_view(WorkspaceView::Conversation(first), window, cx);
            assert_eq!(this.pane_id, 1);
            assert_eq!(this.view.displayed_session(), Some(first));
            assert_eq!(
                this.saved_pane_layout().root.panes(),
                [(1, Some(701)), (other_pane, Some(702))]
            );
            assert_eq!(
                this.conversation.composer.read(cx).value().as_ref(),
                "first pane draft"
            );
            this.activate_pane(other_pane, cx);
            this.close_pane(window, cx);

            // Archived sessions disappear from their pane without affecting
            // another session or an agent process.
            this.projects[0].agents[0].config.archived = true;
            this.render_panes(window, cx);
            assert_eq!(this.view, WorkspaceView::Empty);
            this.restore_panes(Some(saved.clone()), window, cx);
            assert_eq!(this.pane_layout.focused, 3);
            assert_eq!(
                this.saved_pane_layout().root.panes(),
                [(2, None), (3, Some(702))]
            );
            assert_eq!(this.view.displayed_session().unwrap().agent_index, 1);
            this.close_pane(window, cx);
            assert_eq!(this.view, WorkspaceView::Empty);
        });
    });
}
